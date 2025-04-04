/// Convenience functions associated with the parsing of INI and CLI arguments.
mod utils;

/// Convenience Type definitions associated with properties during the lifetime of the experiment.
pub(crate) mod typedef;

use crate::{Error, Result};
use configparser::ini;
use std::{
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
            skip_num: 0,
            speed: SamplingSpeed::High,
            encode: Encode::Binary,

            gain: None,
            filter_const: None,
            verbosity: 0,

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

    pub fn with_file(&mut self, file: &Path) -> Result<()> {
        let mut config = ini::Ini::new();
        config.load(file).unwrap();
        self.with_config(config)
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
                )))
            }
            Ok(None) => return Err(Error::ini_missing("Path", "save_to_file")),
            Ok(Some(t)) => t,
        };

        Ok(())
    }

    /// This the frequency in Hertz at which data is being emitted from the board after skip_num (`s`)
    /// decimation.
    pub fn sampling_frequency(&self) -> f64 {
        self.speed.base_sampling_freq() as f64 / (1. + self.skip_num as f64)
    }
}
