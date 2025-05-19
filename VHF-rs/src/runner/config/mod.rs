/// Convenience functions associated with the parsing of INI and CLI arguments.
mod utils;

/// Convenience Type definitions associated with properties during the lifetime of the experiment.
pub(crate) mod typedef;

use super::fold::StreamFold;
use crate::{Error, Result};
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
        self.speed = SamplingSpeed::from_str(
            config
                .get("Board", "speed")
                .unwrap_or(Configs::default().speed.to_string())
                .as_str(),
        )?;

        self.encode = Encode::from_str(
            config
                .get("Board", "encode")
                .unwrap_or(Configs::default().encode.to_string())
                .as_str(),
        )?;

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
        match utils::get_with_ext_interp(&config, "Paths", "save_dir") {
            Ok(save_dir) => self.save_dir = PathBuf::from(save_dir),
            Err(_) => log::warn!("No save directory provided by INI file. Using default."),
        };
        match utils::get_with_ext_interp(&config, "Paths", "board") {
            Ok(board) => self.board = PathBuf::from(board),
            Err(_) => log::warn!("No board provided by INI file. Using default."),
        };
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

    /// Checks if configuration has tripped anything. Errors only if warnings have been emitted.
    fn validate_config(&self) -> Result<()> {
        if self.skip_num + 1 < 10 {
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

        result.push("-".to_string() + &self.speed.to_string());
        result.push("-".to_string() + &self.encode.to_string());

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
