use crate::{Error, Result};

/// VHF Board collecting data in 10MHz or 20MHz mode.
#[derive(Copy, Clone, Debug)]
pub enum SamplingSpeed {
    Low,
    High,
}

impl ToString for SamplingSpeed {
    fn to_string(&self) -> String {
        match self {
            SamplingSpeed::High => "high",
            SamplingSpeed::Low => "low",
        }
        .to_string()
    }
}

impl TryFrom<String> for SamplingSpeed {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self> {
        match value.chars().next() {
            None => Err(Error::new("Empty String")),
            Some('l') => Ok(SamplingSpeed::Low),
            Some('h') => Ok(SamplingSpeed::High),
            Some(_) => Err(Error::new("Unrecognised Input")),
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

impl TryFrom<String> for Encode {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self> {
        match &value[..3] {
            "asc" => Ok(Encode::ASCII),
            "bin" => Ok(Encode::Binary),
            "hex" => Ok(Encode::Hexadecimal),
            "tex" => Ok(Encode::ASCII),
            "txt" => Ok(Encode::ASCII),
            _ => Err(Error::new("Unrecognised Input")),
        }
    }
}
