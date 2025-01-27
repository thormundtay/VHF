use core::error;
use std::fmt;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    EvalExpr(evalexpr::EvalexprError),
    IniParse(String),
    IniMissing(String),
    ParseEmpty,
    ParseUnrecognised(String),
    CIo(nix::errno::Errno),
    Io(std::io::Error),
    MMap(mmap_rs::Error),
    Ioctl(nix::errno::Errno),
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error {
    pub fn ini_missing(section: &str, key: &str) -> Self {
        Error::IniMissing(format!("ini file '{section} - {key}' could not be found"))
    }

    pub fn ini_coerce(section: &str, key: &str, typing: &str) -> Self {
        Error::IniParse(format!(
            "ini file '{section} - {key}' could not be coerced to {typing}"
        ))
    }
}
