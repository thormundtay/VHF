/// Convenience functions associated with the parsing of INI and CLI arguments.
mod utils;

/// Convenience Type definitions associated with properties during the lifetime of the experiment.
mod typedef;

use crate::{Error, Result};
use configparser::ini;
use std::path::{Path, PathBuf};
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
}

impl Default for Configs {
    /// This represents the set of arguments run by teststream.c/stream.rs without any options.
    fn default() -> Self {
        Self {
            num_samples: 0,
            skip_num: 0,
            speed: SamplingSpeed::High,
            encode: Encode::Binary,
        }
    }
}

impl Configs {
    pub fn new(file: Option<PathBuf>) -> Result<Self> {
        let mut result = Self::default();
        // Take the string parsed out of configparser to populate all the relevant properties.
        if file.is_some() {
            Self::from_file(&mut result, file.unwrap().as_path())?;
        }
        Ok(result)
    }

    pub fn from_file(&mut self, file: &Path) -> Result<()> {
        let mut config = ini::Ini::new();
        config.load(file).unwrap();
        self.from_config(config)
    }

    // Assumes ExtendedInterpolation from Python's ConfigParser
    fn from_config(&mut self, config: ini::Ini) -> Result<()> {
        // Section: Board
        if let evalexpr::Value::Int(num_samples) = config
            .get("Board", "num_samples")
            .unwrap_or("0".to_string())
            .eval()?
        {
            self.num_samples = usize::try_from(num_samples).unwrap();
        } else {
            return Err(Error::new(
                "ini file 'Board - num_samples' not coercible into a usize.",
            ));
        };
        if let evalexpr::Value::Int(skip_num) = config
            .get("Board", "skip_num")
            .unwrap_or("0".to_string())
            .eval()?
        {
            self.skip_num = skip_num as u16;
        } else {
            return Err(Error::new(
                "ini file 'Board - skip_num' not coercible into a u16.",
            ));
        }
        self.speed = match config
            .get("Board", "speed")
            .unwrap_or(Configs::default().speed.to_string())
            .try_into()
        {
            Ok(s) => s,
            Err(_) => {
                return Err(Error::new(
                    "ini file 'Board - skip' not coercible into SamplingSpeed.",
                ))
            }
        };

        self.encode = match config
            .get("Board", "encode")
            .unwrap_or(Configs::default().encode.to_string())
            .try_into()
        {
            Ok(t) => t,
            Err(_) => {
                return Err(Error::new(
                    "ini file 'Board - encode' not coercible into Encode.",
                ))
            }
        };

        Ok(())
    }
}
