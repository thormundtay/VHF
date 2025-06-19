/// Convenience functions associated with the parsing of INI and CLI arguments.
mod utils;

/// Convenience Type definitions associated with properties during the lifetime of the experiment.
pub(crate) mod typedef;

use super::fold::StreamFold;
use crate::{Error, Result};
use clap::{Arg, ArgAction, ArgGroup, Command, ValueHint, value_parser};
use configparser::ini;
use std::{
    collections::HashMap,
    ffi::{CString, OsString},
    path::{Path, PathBuf},
    str::FromStr,
};
use typedef::*;
use utils::PythonMath;

/// Parameters used to run VHF board.
///
/// We expect to first source from the INI file, before overwriting with any command line
/// arguments. The implementation here however does not impose any strict ordering.
///
/// While trying to be inline with Rust's impossible to represent invalid states, the struct should
/// only contain known data, the non-finalization means that the state within the struct is an
/// over-representation of what is alloweable. This is a superset of the valid [BoardConfig],
/// and [WriterBuilder] states.
#[derive(Clone, Debug)]
pub struct Configs {
    /// For a single continuous file, this is the number of samples expected to be at least within
    /// the file.
    pub num_samples: usize,
    /// This is the number of continuous files is expected to run without calling USB_START_ENGINE
    /// again.
    pub num_files: usize,
    /// This value (known as "-s skipnum") is passed into the FPGA for decimation. Adding 1 to it
    /// yields the decimation factor by the FPGA. Valid in 0..=65535.
    pub skip_num: u16,
    /// This value (known as "-h" or "-l") is passed into the FPGA to operate at either 20 or 10
    /// MHz. Defaults to High unless otherwise specified.
    pub speed: SamplingSpeed,
    /// The manner by which (I, Q, M) data is stored into the file.
    pub encode: Encode,

    /// This is the dynamic gain (-g)
    pub gain: Option<u8>,
    /// This is the hardware filter (-F) used by the FPGA for low pass filtering.
    pub filter_const: Option<u8>,
    pub verbosity: u8,

    /// Runtime processing method
    pub stream_fold: StreamFold,

    /// Other information about the phasemeter not related to the operation of the board.
    pub phasemeter_kwargs: HashMap<CString, CString>,

    // base_dir // removed because we do not call into ./teststream.exec (or whatever)
    /// Location where files should be written to
    pub save_dir: PathBuf,
    pub board: PathBuf,
    pub save_to_file: bool,
}

impl Default for Configs {
    /// This represents the set of arguments run by teststream.c/stream.rs without any options.
    fn default() -> Self {
        Self {
            num_samples: 0,
            num_files: 1,
            skip_num: 0,
            speed: SamplingSpeed::High,
            encode: Encode::Binary,

            gain: None,
            filter_const: None,
            verbosity: 0,

            stream_fold: StreamFold::none_default(),

            phasemeter_kwargs: HashMap::new(),

            save_dir: PathBuf::from("./Data"),
            board: PathBuf::from("/dev/usbhybrid0"),
            save_to_file: false,
        }
    }
}

impl Configs {
    pub fn new(file: Option<PathBuf>) -> Result<Self> {
        let mut result = Self::default();
        // Take the string parsed out of configparser to populate all the relevant properties.
        if file.is_some() {
            Self::with_file(&mut result, file.unwrap().as_path())?;
        }
        Ok(result)
    }

    /// Builds configuration from file.
    /// This is destructive on the existing state of Configuration.
    pub fn with_file(&mut self, file: &Path) -> Result<()> {
        let mut config = ini::Ini::new();
        config.load(file).unwrap();
        let result = self.with_config(config);
        if result.is_ok() {
            let _ = self.validate_config(true, false);
        }
        result
    }

    // Assumes ExtendedInterpolation from Python's ConfigParser
    fn with_config(&mut self, config: ini::Ini) -> Result<()> {
        // Section: Board
        if let evalexpr::Value::Int(num_samples) = config
            .get("Board", "num_samples")
            .unwrap_or("0".to_string())
            .eval()?
        {
            self.num_samples = usize::try_from(num_samples)
                .map_err(|_| Error::ini_coerce("Board", "num_samples", "usize"))?
        };
        if let evalexpr::Value::Int(num_files) = config
            .get("Extended Sampling", "num_runs")
            .unwrap_or("0".to_string())
            .eval()?
        {
            self.num_files = usize::try_from(num_files)
                .map_err(|_| Error::ini_coerce("Extended Sampling", "num_runs", "usize"))?
        };
        if let evalexpr::Value::Int(skip_num) = config
            .get("Board", "skip_num")
            .unwrap_or("0".to_string())
            .eval()?
        {
            self.skip_num = u16::try_from(skip_num)
                .map_err(|_| Error::ini_coerce("Board", "skip_num", "u16"))?
        };
        self.speed = config
            .get("Board", "speed")
            .ok_or_else(|| {
                log::warn!("Board speed not found in config; Using default.");
                Error::ParseEmpty
            })
            .and_then(|e| SamplingSpeed::from_str(e.as_str()))
            .unwrap_or(Self::default().speed);

        self.encode = config
            .get("Board", "encode")
            .ok_or_else(|| {
                log::warn!("Board encode not found in config; Using default.");
                Error::ParseEmpty
            })
            .and_then(|e| Encode::from_str(e.as_str()))
            .unwrap_or(Self::default().encode);

        self.gain = utils::if_enabled_value(&config, "Board", "vga_num", |v| v <= 8)?;
        self.filter_const = utils::if_enabled_value(&config, "Board", "filter_const", |v| v <= 15)?;

        self.verbosity = match config.getuint("Board", "v").map_err(Error::IniParse)? {
            None => return Err(Error::ini_missing("Board", "v")),
            Some(t) => {
                if t <= 15 {
                    t as u8
                } else {
                    return Err(Error::IniParse(
                        "ini file 'Board - v' out of bounds.".to_string(),
                    ));
                }
            }
        };

        // Section: Runtime processing
        self.stream_fold =
            match config.get("Software Processing".to_ascii_lowercase().as_str(), "type") {
                None => Ok(StreamFold::identity_default()),
                Some(mode) => match mode.as_str() {
                    "None" => Ok(StreamFold::identity_default()),
                    _ => Err(Error::ini_missing("Software Processing", "type")),
                },
            }?;

        // Section: Phasemeter details
        self.phasemeter_kwargs = {
            let tmp = config
                .get_map()
                .ok_or(Error::ini_missing("FILE", "VALUE"))?;
            log::trace!("config.get_map = {:?}", &tmp);
            let kv: &HashMap<_, _> = tmp
                .get("Phasemeter Details".to_ascii_lowercase().as_str())
                .ok_or(Error::ini_missing("FILE", "Phasemeter Details"))?;
            HashMap::from_iter(kv.iter().map(|(k, v)| {
                let v = v.clone();
                (
                    CString::new(k.as_str()).unwrap(),
                    CString::new(v.unwrap_or_default()).unwrap(),
                )
            }))
        };

        // Section: Paths
        self.save_dir = utils::get_with_ext_interp(&config, "Paths", "save_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|e| {
                log::warn!("No save directory provided by INI file. Using default. Error: {e}");
                Self::default().save_dir
            });
        self.board = utils::get_with_ext_interp(&config, "Paths", "board")
            .and_then(|p| PathBuf::from(p).canonicalize().map_err(Error::Io))
            .unwrap_or_else(|e| {
                log::warn!("No board provided by INI file. Using default. Error: {e}");
                Self::default().board
            });
        self.save_to_file = match config.getbool("Paths", "save_to_file") {
            Err(e) => {
                return Err(Error::IniParse(format!(
                    "ini file 'Path - save_to_file' parse failed: {e}"
                )));
            }
            Ok(None) => return Err(Error::ini_missing("Path", "save_to_file")),
            Ok(Some(t)) => t,
        };

        Ok(())
    }

    /// Modifies by arguments as taken from std::env::args_os().
    ///
    /// This is destructive on the existing state of Configuration.
    /// # Panic
    /// Panics from [clap::Command::get_matches_from].
    // Parsing of clap_args() is in the order of the [self] struct.
    // args.get_one(/* ... */) argument comes from
    // This function thus does mapping from args variable to [self] struct with minimal bounds
    // checking, deferring to other functions to validate check.
    pub fn with_cli(
        &mut self,
        env_args: impl IntoIterator<Item = OsString> + std::fmt::Debug,
    ) -> Result<()> {
        log::debug!("with_cli called with env_args: {:?}", env_args);
        let args = self.clap_args().get_matches_from(env_args);

        if let Some(&num_samples) = args.get_one::<usize>("Number of samples") {
            self.num_samples = num_samples;
        }
        if let Some(&num_files) = args.get_one::<usize>("Number of files") {
            self.num_files = num_files;
        }
        if let Some(&skip) = args.get_one::<u16>("Board Skip Num") {
            self.skip_num = skip;
        }

        if args.get_flag("speed_low") {
            self.speed = SamplingSpeed::Low;
        } else if args.get_flag("speed_high") {
            self.speed = SamplingSpeed::High;
        }
        if args.get_flag("binary") {
            self.encode = Encode::Binary;
        } else if args.get_flag("ASCII") {
            self.encode = Encode::ASCII;
        } else if args.get_flag("hexadecimal") {
            self.encode = Encode::Hexadecimal;
        }

        if args.get_one::<u8>("Board Gain").is_some_and(|&v| v <= 8) {
            // Condition duplicated from [self::with_config]
            self.gain = args.get_one::<u8>("Board Gain").cloned();
        } else if args.get_one::<u8>("Board Gain").is_some() {
            log::warn!("Board Gain value given out of bounds! Ignored.")
        };
        if args
            .get_one::<u8>("Board Filter Constant")
            .is_some_and(|&v| v <= 15)
        {
            // Condition duplicated from [self::with_config]
            self.filter_const = args.get_one::<u8>("Board Filter Constant").cloned();
        } else if args.get_one::<u8>("Board Filter Constant").is_some() {
            log::warn!("Board Filter Constant value given out of bounds! Ignored.")
        };
        if let Some(&verbosity) = args.get_one::<u8>("verbosity") {
            self.verbosity = verbosity;
        }
        // self.stream_fold = ...

        if let Some(phasemeter_kwargs) = args.get_raw("File details") {
            // First convert OsString into CString
            let kv = phasemeter_kwargs.map(|raw_value| -> (CString, CString) {
                // Account for keys without values
                let rv = raw_value.to_str().expect("Invalid UTF-8 in --phasemeter.");
                let (k, v) = rv
                    .split_at_checked(rv.find('=').unwrap_or(rv.len()))
                    .map(|(r, v)| (r, v.as_bytes().iter().skip(1).cloned().collect()))
                    .unwrap_or((rv, vec![]));
                (
                    CString::new(k).expect("Could not make CString"),
                    CString::new(v).expect("Could not make CString"),
                )
            });

            // Insert if key does not already exist.
            kv.for_each(|(k, v)| {
                match self.phasemeter_kwargs.entry(k.clone()) {
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        log::warn!("Overwriting phasemeter kwarg `{:?}` with `{:?}`.", &k, &v);
                        *entry.into_mut() = v;
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(v);
                    }
                };
            });
        }

        // We still support -o even if deprecated.
        if let Some(save_loc) = args.get_one::<PathBuf>("outfile") {
            log::warn!("-o flag has been deprecated! Please use --save_dir!");
            if save_loc.extension().is_some() {
                // If there is an extension, means its a file-like path, and take parent.
                // leave for [self::validate_config] to check
                self.save_dir = save_loc
                    .parent()
                    .expect("with extension implies file implies in directory")
                    .to_path_buf();
            } else if save_loc.is_file() {
                log::error!(
                    "-o value: {} points to an existing file!",
                    save_loc.display()
                );
                return Err(Error::User);
            } else if save_loc.is_dir() {
                // If its a specified folder already on disk, that's great
                self.save_dir = save_loc.canonicalize().map_err(Error::Io)?
            } else {
                // This is folder-like, we use it and let [self::validate_config] check. Do not
                // canonicalize as it will error out
                self.save_dir = save_loc.clone();
            }
            self.validate_config_path(true)?; // Do not let self.save_dir be non-canonicalized upon scope-end
        }
        // Overwrite -o input if -D is also provided
        if let Some(save_loc) = args.get_one::<PathBuf>("save_dir") {
            // Check that specified is folder-like
            if save_loc.extension().is_some() {
                // If there is an extension, means its a file-like path, and take parent.
                // leave for [self::validate_config] to check
                log::warn!("Please pass only directory-like paths!");
                self.save_dir = save_loc
                    .parent()
                    .expect("with extension implies file implies in directory")
                    .to_path_buf();
            } else if save_loc.is_file() {
                log::error!(
                    "-D value: {} points to an existing file! Expected directory!",
                    save_loc.display()
                );
                return Err(Error::User);
            } else if save_loc.is_dir() {
                // If its a specified folder already on disk, that's great
                self.save_dir = save_loc.canonicalize().map_err(Error::Io)?
            } else {
                // This is folder-like, we use it and let [self::validate_config] check. Do not
                // canonicalize as it will error out
                self.save_dir = save_loc.clone();
            }
            self.validate_config_path(true)?; // Do not let self.save_dir be non-canonicalized upon scope-end
        }
        if let Some(b_in) = args.get_one::<PathBuf>("VHF board") {
            self.board = b_in.canonicalize().unwrap_or(b_in.to_path_buf());
            self.validate_config_path(true)?; // Do not let self.board be non-canonicalized upon scope-end
        };

        Ok(())
    }

    /// For user input by stream.rs
    fn clap_args(&mut self) -> clap::Command {
        let mut cmd = Command::new("VHF Stream").disable_help_flag(true);
        cmd = cmd
            .about("Stream data from VHF board.")
            .long_about("Successor of `svn:usbhybrid/apps/teststream.c` streaming executable. This program allows for in-flight processing, and guarantees no reset between file instances.\n\nArguments are first taken from `./VHF_board_params.ini` before being overwritten by any flags passed into this executable.");

        // -?,--help
        cmd = cmd.arg(
            Arg::new("help")
                .short('?')
                .long("help")
                .action(ArgAction::Help),
        );

        // -U <device>
        cmd = cmd.arg(
            Arg::new("VHF board")
                .short('U')
                .long("board")
                .action(ArgAction::Set)
                .value_hint(ValueHint::FilePath)
                .value_parser(value_parser!(PathBuf))
                .help("Board to read from")
                .long_help(
                    "Board to read from. Boards accessible can be determined from ./clear_FIFO",
                ),
        );

        // -q <num_samples> --num-files <num_files>
        cmd = cmd.arg(
            Arg::new("Number of samples")
                .short('q')
                .long("num_samples")
                .action(ArgAction::Set)
                .value_parser(value_parser!(usize))
                .help("Number of continuous samples per file")
                .long_help(
                    "Number of continuous samples per file.\n\
                    Setting 0 to not save to file for continuous streaming not yet supported.",
                ),
        );
        cmd = cmd.arg(
            Arg::new("Number of files")
                .long("num_files")
                .action(ArgAction::Set)
                .value_parser(value_parser!(usize))
                .help("Number of save files")
                .long_help(
                    "Number of save files.\n\
                    (Not yet implemented: If num_samples is set to 0, this argument is ignored.)",
                ),
        );

        // [-l|-h]
        cmd = {
            let low_speed = Arg::new("speed_low")
                .short('l')
                .action(ArgAction::SetTrue)
                .help("Low sampling speed")
                .long_help("VHF board samples at 80MHz, each (IQM) value is estimated from 8 samples, yielding a base sampling rate of 10 MHz.");
            let high_speed = Arg::new("speed_high")
                .short('h')
                .action(ArgAction::SetTrue)
                .help("High sampling speed")
                .long_help("VHF board samples at 80MHz, each (IQM) value is estimated from 4 samples, yielding a base sampling rate of 20 MHz.");
            let speed_group = ArgGroup::new("Speed")
                .multiple(false)
                .arg("speed_low")
                .arg("speed_high");
            cmd.arg(low_speed).arg(high_speed).group(speed_group)
        };

        // -F -g -s
        cmd = cmd.arg(
            Arg::new("Board Filter Constant")
                .short('F')
                .long("filter")
                .action(ArgAction::Set)
                .value_parser(value_parser!(u8))
                .help("Hardware filter for low pass filtering")
                .long_help("Hardware filter for low pass filtering. Valid: 0..=15"),
        );
        cmd = cmd.arg(
            Arg::new("Board Gain")
                .short('G')
                .long("gain")
                .action(ArgAction::Set)
                .value_parser(value_parser!(u8))
                .help("Dynamic gain")
                .long_help("Dynamic gain. Valid: 0..=8"),
        );
        cmd = cmd.arg(
            Arg::new("Board Skip Num")
                .short('s')
                .long("skip")
                .action(ArgAction::Set)
                .value_parser(value_parser!(u16))
                .help("Decimation factor for FPGA")
                .long_help(
                    "Skip_num is passed into the FPGA for decimation. \
                    Adding 1 to it yields the decimation factor by the FPGA. \
                    Valid: 0..=65535.\nThis is not the total decimation, \
                    if stream-filtering is present.",
                ),
        );

        // [-b|-t|-x]
        cmd = {
            let binary = Arg::new("binary")
                .short('b')
                .long("binary")
                .action(ArgAction::SetTrue)
                .help("Packed binary output")
                .long_help("File will be written in packed binary mode.");
            let ascii = Arg::new("ASCII")
                .short('t')
                .long("text")
                .action(ArgAction::SetTrue)
                .help("ASCII output")
                .long_help("File will be written in ASCII mode.");
            let hexadecimal = Arg::new("hexadecimal")
                .short('x')
                .long("hexadecimal")
                .action(ArgAction::SetTrue)
                .help("Hexadecimal output")
                .long_help("File will be written in Hexadecimal mode.");
            let encode_group =
                ArgGroup::new("Encode")
                    .multiple(false)
                    .args(["binary", "ASCII", "hexadecimal"]);
            cmd.args([binary, ascii, hexadecimal]).group(encode_group)
        };

        // -v
        cmd = cmd.arg(
            Arg::new("verbosity")
                .short('v')
                .long("verbosity")
                .action(ArgAction::Set)
                .value_parser(value_parser!(u8))
                .help("Verbosity level of file output.")
                .long_help(
                    "Sets the verbosity level of the file output. Add the values for adding options.\n\
                    0: No Header is added (default).\n\
                    1: A string representation of commands run is added.\n\
                    2: Date and time associated to first data point in the data file.\n\
                    4: Use v2 (or higher) file output. Necessary for putting if filtering was used (or not) in header data.\n\
                       (Not yet implemented: Also see `--net_cdf`.)\n\
                    8: Header also includes m-overflow indices. Requires v2 binary file format from 4.",
                ),
        );

        // -o
        cmd = cmd.arg(
            Arg::new("outfile")
                .short('o')
                .long("outfile")
                .action(ArgAction::Set)
                .value_hint(ValueHint::FilePath)
                .value_parser(value_parser!(PathBuf))
                .help("[Deprecated] File to save to.")
                .long_help(
                    "[Deprecated] File path to save to. Program will instead take the parent directory \
                    and automatically generate the name.",
                )
        );
        cmd = cmd.arg(
            Arg::new("save_dir")
                .short('D')
                .long("save_dir")
                .action(ArgAction::Set)
                .value_hint(ValueHint::DirPath)
                .value_parser(value_parser!(PathBuf))
                .help("Directory to save to."),
        );

        // --phasemeter
        cmd = cmd.arg(
            Arg::new("File details")
                .long("phasemeter")
                .action(ArgAction::Append) // Allows multiple --phasemeter-kwarg key=value
                .num_args(0..)
                .value_name("DETAIL=VALUE")
                .help("Phasemeter details for filename")
                .long_help(
                    "Phasmeter details to save in the filename. E.g.: `./stream --phasemeter fibre_length=1.0km laser_chip=ULN00238`"
                )
        );

        cmd
    }

    /// Used to ensure [Configs] is valid before running VHF board.
    pub fn is_valid(&mut self) -> Result<()> {
        self.validate_config(true, true)
    }

    /// Checks if configuration has tripped anything. Errors if strict and if warnings have been emitted.
    // Typically the first pass of ini_config_file's path should not error out, as CLI after might fix.
    // Thus, call strict only for finalisation of config.
    fn validate_config(&mut self, strict: bool, strict_path: bool) -> Result<()> {
        if self.skip_num + 1 < 5 {
            log::warn!("Received skip_num less than 10! byte alignment has known to break!");
            if strict {
                return Err(Error::ini_coerce("Board", "skip_num", "less than 10"));
            }
        }

        if self.verbosity >= 4 && (self.verbosity & 0b100 == 0) && strict {
            log::warn!(
                "Verbosity was found to have been configured to use v2 (or higher) file writer without specifying use of v2 (or higher) file writer. Coercing."
            );
            self.verbosity |= 0b100;
        }

        self.validate_config_path(strict_path)?;

        Ok(())
    }

    /// Checks if specified folder locations exists. Makes best effort to create folder.
    /// Errors out if fails to create folder and strict.
    fn validate_config_path(&mut self, strict: bool) -> Result<()> {
        // Error only if not in /dev. Warns if board not found.
        if self.board.is_symlink() {
            // is symlink because passed via CLI rather than resolved when reading from file
            if let Ok(board_loc) = self.board.canonicalize() {
                self.board = board_loc
            } else {
                log::warn!("Failed to canonicalize {}", self.board.display());
            }
        }
        if !self.board.starts_with("/dev") {
            log::error!(
                "self.board expected to be in /dev, found in {}",
                self.board.display()
            );
            return Err(Error::InternalInconsistency);
        } else if !self.board.exists() {
            log::warn!(
                "self.board at `{}` could not be found.",
                self.board.display()
            );
        }

        // Ensure is a full path that is an existing directory
        if self.save_dir.is_relative() || !self.save_dir.exists() {
            self.save_dir = match self.save_dir.canonicalize() {
                Ok(d) => d,
                Err(e) => {
                    log::warn!(
                        "Could not canonicalize, trying to create self.save_dir = `{}`. Error msg = {e}",
                        self.save_dir.display()
                    );
                    match strict {
                        true => {
                            std::fs::create_dir_all(&self.save_dir).map_err(Error::Io)?;
                            self.save_dir.canonicalize().map_err(Error::Io)?
                        }
                        false => {
                            let _ = std::fs::create_dir_all(&self.save_dir);
                            self.save_dir.canonicalize().unwrap_or_else(|e| {
                                log::warn!("Still could not canonicalize. Error msg = {e}");
                                self.save_dir.clone()
                            })
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// This the frequency in Hertz at which data is being emitted from the board after skip_num (`s`)
    /// decimation.
    pub fn sampling_frequency(&self) -> f64 {
        self.speed.base_sampling_freq() as f64 / (1. + self.skip_num as f64)
    }

    /// This determines the time difference the first data point of multiple files.
    pub fn file_timespan(&self) -> jiff::Span {
        // In case there are drifts...
        log::debug!(
            "Timespan of one sample point in nanoseconds = {}",
            1e9 / self.sampling_frequency()
        );
        self.num_samples as i64
            * jiff::Span::new()
                .try_nanoseconds((1e9 / self.sampling_frequency()).round() as i64)
                .unwrap()
    }

    /// This is a string representation of what the C variant would have received from the command
    /// line. This primarily is used just to keep track of experiment properties.
    pub fn details(&self) -> String {
        let mut result = Vec::with_capacity(16);

        result.push("-U".to_string());
        result.push(self.board.clone().into_os_string().into_string().unwrap());

        result.push("-q".to_string());
        result.push(self.num_samples.to_string());

        result.push("-s".to_string());
        result.push(self.skip_num.to_string());

        result.push("-".to_string() + self.speed.as_char());
        result.push("-".to_string() + self.encode.as_char());

        if let Some(filter_const) = self.filter_const {
            result.push("-F".to_string());
            result.push(filter_const.to_string());
        }

        if let Some(gain_const) = self.gain {
            result.push("-G".to_string());
            result.push(gain_const.to_string());
        }

        result.push("-v".to_string());
        result.push(self.verbosity.to_string());

        result.push("-o".to_string());
        result.push(self.filename());

        result.join(" ")
    }

    /// This is a string representation of the filename after the timestamp as in the filesystem.
    pub fn filename(&self) -> String {
        let s: Vec<String> = {
            let mut tmp: Vec<_> = self.phasemeter_kwargs.iter().collect();
            tmp.sort();
            tmp.into_iter()
                .map(|(a, b)| {
                    [
                        a.clone().into_string().unwrap(),
                        b.clone().into_string().unwrap(),
                    ]
                    .join("_")
                    .to_string()
                })
                .collect()
        };
        s.join("_")
    }

    /// Prints user-friendly string as to the configuration being used to run.
    pub fn inform_params(&self) {
        const BLUE: &str = "\x1B[34m";
        const REDBOLD: &str = "\x1B[31;1m";
        const RESET: &str = "\x1B[0m";

        let sf = self.sampling_frequency();
        if sf < 1e3 {
            println!("Sampling at {:.4} Hz.", sf);
        } else if sf < 1e6 {
            println!("Sampling at {:.4} kHz.", sf / 1e3);
        } else if sf < 1e9 {
            println!("Sampling at {:.4} MHz.", sf / 1e6);
        } else {
            println!("Sampling at {sf} Hz.");
        }

        if let Some(filter_const) = self.filter_const {
            println!(
                "Filter constant has been set to: {BLUE}{}{RESET}",
                filter_const
            );
        }

        if let Some(gain_const) = self.gain {
            println!("Onboard gain has been set to: {BLUE}{}{RESET}", gain_const);
        }

        println!(
            "Phasemeter details used: {BLUE}{:?}{RESET}",
            self.phasemeter_kwargs
        );

        if self.save_to_file {
            println!(
                "Output will be written to {BLUE}{:?}{RESET}.",
                self.save_dir
            );
        } else {
            println!("Output will be captured from {BLUE}STDIN{RESET}.");
        }

        let total_time = self.num_files as i64 * self.file_timespan();
        println!(
            "Sampling is expected to take {REDBOLD}{}{RESET}.",
            total_time
                .round(
                    jiff::SpanRound::new()
                        .smallest(jiff::Unit::Second)
                        .largest(jiff::Unit::Day)
                        .relative(jiff::SpanRelativeTo::days_are_24_hours())
                )
                .unwrap_or_default()
        )
    }

    /// If all parameters in [self] are valid, a [BoardConfig] will be produced that fully
    /// represents all necessary processes required to interact with VHF Board.
    pub fn build_board_config(&self) -> Result<BoardConfig> {
        // Check self.board resolves and exists. Done here because it is Configs that does not
        // fully assert the correctness.
        #[cfg(not(test))]
        if !self.board.canonicalize().map_err(Error::Io)?.exists() {
            log::error!("Board could not be found!");
            return Err(Error::User);
        };

        BoardConfig::new(self)
    }
}

/// Valid representation of board interaction along with process requirements.
/// (These are placed together as the board has to collect more data in the event that process
/// decimates the board's collected data.)
pub struct BoardConfig<'a> {
    /// For a single continuous file, this is the number of samples expected to be at least within
    /// the file.
    pub num_samples: &'a usize,
    /// This is the number of continuous files is expected to run without calling USB_START_ENGINE
    /// again.
    pub num_files: &'a usize,
    /// This value (known as "-s skipnum") is passed into the FPGA for decimation. Adding 1 to it
    /// yields the decimation factor by the FPGA. Valid in 0..=65535.
    pub skip_num: &'a u16,
    /// This value (known as "-h" or "-l") is passed into the FPGA to operate at either 20 or 10
    /// MHz. Defaults to High unless otherwise specified.
    pub speed: &'a SamplingSpeed,

    /// This is the dynamic gain (-g)
    pub gain: &'a Option<u8>,
    /// This is the hardware filter (-F) used by the FPGA for low pass filtering.
    pub filter_const: &'a Option<u8>,

    /// Runtime processing method
    pub stream_fold: &'a StreamFold,

    pub board: &'a PathBuf,
}

impl<'a> BoardConfig<'a> {
    /// It is strongly recommended that [Configs::is_valid] is called before this point.
    fn new(config: &'a Configs) -> Result<Self> {
        Ok(BoardConfig {
            num_samples: &config.num_samples,
            num_files: &config.num_files,
            skip_num: &config.skip_num,
            speed: &config.speed,
            gain: &config.gain,
            filter_const: &config.filter_const,
            stream_fold: &config.stream_fold,
            board: &config.board,
        })
    }

    /// Number of elements to read from VHF board, after board-decimation factor, before any
    /// processing by us.
    pub fn total_elements_to_read(&self) -> usize {
        self.num_files * self.num_samples
    }

    /// Gets the parameters of StreamFold part of the configuration.
    pub fn stream_fold_parameters(&self) -> &StreamFold {
        &self.stream_fold
    }
}

#[cfg(test)]
mod configurations {
    use super::*;

    #[test]
    fn intended_file_case() {
        let ini_config = {
            let mut config = ini::Ini::new();
            config
                .read(
                    "[Board]
                    num_samples = 1
                    skip_num = 10 - 1
                    speed: low
                    encode: binary
                    vga_num = 0
                    vga_num_enable = False
                    filter_const = 0
                    filter_const_enable = False
                    v = 3

                    [Paths]
                    base_dir: .
                    save_dir: Data
                    # This should just yield the default
                    board: ${base_dir}/vhf_board
                    save_to_file = True

                    [Phasemeter Details]"
                        .to_string(),
                )
                .expect("Ini lib parse error.");
            config
        };

        let mut config = Configs::new(None).expect("Unable to create Config from empty");

        // There are no missing fields, and therefore should not raise errors.
        let p = config.with_config(ini_config);
        assert!(p.is_ok());

        assert_eq!(config.num_samples, 1);
        assert_eq!(config.skip_num, 10 - 1);
        matches!(config.speed, SamplingSpeed::Low);
        matches!(config.encode, Encode::Binary);
        assert_eq!(config.gain, None);
        assert_eq!(config.filter_const, None);
        assert_eq!(config.verbosity, 3);

        assert_eq!(config.save_dir, PathBuf::from("Data"));
        // Ignore due to canonicalization
        assert_eq!(config.board, Configs::default().board);
    }

    #[test]
    fn intended_cli_case() {
        let args: Vec<OsString> = [
            "exec_name",
            "-U",
            "/dev/usbhybrid99",
            "-q",
            "524288",
            "--num_files",
            "500",
            "-l",
            "-s",
            "499",
            "-F",
            "7",
            "-G",
            "3",
            "-b",
            "-v",
            "2",
            "--save_dir",
            std::env::temp_dir().to_str().unwrap(),
            "--phasemeter",
            "length=2km",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();

        let mut config = Configs::default();
        let p = config.with_cli(args);

        assert!(p.is_ok());
        assert_eq!(config.board, PathBuf::from("/dev/usbhybrid99"));
        assert_eq!(config.num_samples, 524288);
        assert_eq!(config.num_files, 500);
        matches!(config.speed, SamplingSpeed::Low);
        assert_eq!(config.filter_const, Some(7));
        assert_eq!(config.gain, Some(3));
        matches!(config.encode, Encode::Binary);
        assert_eq!(config.verbosity, 2);
        assert_eq!(config.save_dir, std::env::temp_dir());
        assert_eq!(config.phasemeter_kwargs.len(), 1);
        assert_eq!(
            config.phasemeter_kwargs.into_iter().next().unwrap(),
            (
                CString::new("length").unwrap(),
                CString::new("2km").unwrap()
            )
        );
    }

    #[test]
    fn overwriting_cli_case() {
        let ini_config = {
            let mut config = ini::Ini::new();
            config
                .read(
                    "[Board]
                    num_samples = 1
                    skip_num = 10 - 1
                    speed: low
                    encode: binary
                    vga_num = 0
                    vga_num_enable = False
                    filter_const = 0
                    filter_const_enable = False
                    v = 3

                    [Paths]
                    base_dir: .
                    save_dir: Data
                    # This should just yield the default
                    board: ${base_dir}/vhf_board
                    save_to_file = True

                    [Phasemeter Details]
                    length=1km"
                        .to_string(),
                )
                .expect("Ini lib parse error.");
            config
        };

        // From file first
        let mut config = Configs::new(None).expect("Unable to create Config from empty");
        let p = config.with_config(ini_config);
        assert!(p.is_ok());
        assert_eq!(
            config.phasemeter_kwargs[&CString::new("length").unwrap()],
            CString::new("1km").unwrap()
        );

        // Overwriting with CLI.
        let args: Vec<OsString> = [
            "exec_name",
            "-U",
            "/dev/usbhybrid99",
            "-q",
            "524288",
            "--num_files",
            "500",
            "-l",
            "-s",
            "499",
            "-F",
            "7",
            "-G",
            "3",
            "-b",
            "--save_dir",
            std::env::temp_dir().to_str().unwrap(),
            "--phasemeter",
            "length=2km",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let p = config.with_cli(args);
        assert!(p.is_ok());

        assert_eq!(config.board, PathBuf::from("/dev/usbhybrid99"));
        assert_eq!(config.num_samples, 524288);
        assert_eq!(config.num_files, 500);
        matches!(config.speed, SamplingSpeed::Low);
        assert_eq!(config.filter_const, Some(7));
        assert_eq!(config.gain, Some(3));
        matches!(config.encode, Encode::Binary);
        assert_eq!(config.save_dir, std::env::temp_dir());
        assert_eq!(config.phasemeter_kwargs.len(), 1);
        assert_eq!(
            config.phasemeter_kwargs.into_iter().next().unwrap(),
            (
                CString::new("length").unwrap(),
                CString::new("2km").unwrap()
            )
        );
    }
}
