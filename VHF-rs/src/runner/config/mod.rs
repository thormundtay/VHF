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
    ffi::CString,
    path::{Path, PathBuf},
    str::FromStr,
};
use typedef::*;
use utils::PythonMath;

/// Parameters used to run VHF board.
///
/// We expect to first source from the INI file, before overwriting with any command line
/// arguments. The implementation here however does not impose any strict ordering.
// In line with Rust's impossible to represent invalid states, the struct should only contain known
// data.
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
            let _ = self.validate_config();
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
                if t <= 5 {
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
            .map(|dir| PathBuf::from(dir))
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

        // -F -G -s
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
                .short('g')
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

        // -o
        cmd = cmd.arg(
            Arg::new("outfile")
                .short('o')
                .long("outfile")
                .action(ArgAction::Set)
                .value_hint(ValueHint::FilePath)
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
                .help("Directory to save to."),
        );

        cmd
    }

    /// Checks if configuration has tripped anything. Errors only if warnings have been emitted.
    fn validate_config(&self) -> Result<()> {
        if self.skip_num + 1 < 5 {
            log::warn!("Received skip_num less than 10! byte alignment has known to break!");
            return Err(Error::ini_coerce("Board", "skip_num", "less than 10"));
        }

        Ok(())
    }

    /// This the frequency in Hertz at which data is being emitted from the board after skip_num (`s`)
    /// decimation.
    pub fn sampling_frequency(&self) -> f64 {
        self.speed.base_sampling_freq() as f64 / (1. + self.skip_num as f64)
    }

    /// Number of elements to read from VHF board, after board-decimation factor, before any
    /// processing by us.
    pub fn total_elements_to_read(&self) -> usize {
        self.num_files * self.num_samples
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

    /// Gets the parameters of StreamFold part of the configuration.
    pub fn stream_fold_parameters(&self) -> &StreamFold {
        &self.stream_fold
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
}

#[cfg(test)]
mod configurations {
    use super::*;

    #[test]
    fn intended_case() {
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
}
