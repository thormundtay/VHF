use crate::{Error, Result};
use std::str::FromStr;

/// VHF Board collecting data in 10MHz or 20MHz mode.
#[derive(Copy, Clone, Debug)]
pub enum SamplingSpeed {
    Low,
    High,
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for SamplingSpeed {
    fn to_string(&self) -> String {
        match self {
            SamplingSpeed::High => "high",
            SamplingSpeed::Low => "low",
        }
        .to_string()
    }
}

impl FromStr for SamplingSpeed {
    type Err = crate::Error;
    fn from_str(value: &str) -> Result<Self> {
        match value.chars().next() {
            None => Err(Error::ParseEmpty),
            Some('l') => Ok(SamplingSpeed::Low),
            Some('h') => Ok(SamplingSpeed::High),
            Some(v) => Err(Error::ParseUnrecognised(format!(
                "SamplingSpeed::try_from got value: {v}"
            ))),
        }
    }
}

impl SamplingSpeed {
    /// This is the sampling frequency in Hertz in the board internal prior to skip-number (`s`) decimation.
    ///
    /// The board samples the analogue signal at 80MHz. To get a singular IQM value, it then
    /// considers either 4 or 8 data points, giving the low and high rate.
    pub fn base_sampling_freq(&self) -> u32 {
        match self {
            SamplingSpeed::High => 20_000_000,
            SamplingSpeed::Low => 10_000_000,
        }
    }

    pub fn in_ns(&self) -> jiff::Span {
        match self {
            SamplingSpeed::High => jiff::Span::new().nanoseconds(50),
            SamplingSpeed::Low => jiff::Span::new().nanoseconds(100),
        }
    }

    /// This is for being in the header
    pub(crate) fn to_char(&self) -> &str {
        match self {
            SamplingSpeed::High => &"h",
            SamplingSpeed::Low => &"l",
        }
    }
}

/// The structure of the file saved.
#[derive(Copy, Clone, Debug)]
pub enum Encode {
    Binary,
    Hexadecimal,
    ASCII,
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for Encode {
    fn to_string(&self) -> String {
        match self {
            Encode::ASCII => "asc",
            Encode::Binary => "bin",
            Encode::Hexadecimal => "hex",
        }
        .to_string()
    }
}

impl FromStr for Encode {
    type Err = crate::Error;
    fn from_str(value: &str) -> Result<Self> {
        match &value[..3] {
            "asc" => Ok(Encode::ASCII),
            "bin" => Ok(Encode::Binary),
            "hex" => Ok(Encode::Hexadecimal),
            "tex" => Ok(Encode::ASCII),
            "txt" => Ok(Encode::ASCII),
            v => Err(Error::ParseUnrecognised(format!(
                "Encode::try_from got value: {v}"
            ))),
        }
    }
}

impl Encode {
    pub(crate) fn to_char(&self) -> &str {
        match self {
            Self::Binary => &"b",
            Self::Hexadecimal => &"x",
            Self::ASCII => &"t",
        }
    }
}
