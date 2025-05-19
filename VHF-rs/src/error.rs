use core::error;
use std::fmt;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    EvalExpr(String),
    IniParse(String),
    IniMissing(String),
    ParseEmpty,
    ParseUnrecognised(String),
    CIo(nix::errno::Errno),
    Io(std::io::Error),
    MMap(mmap_rs::Error),
    Ioctl(nix::errno::Errno),
    IoctlCall(String),
    EngineRunning,
    EngineStopped,
    Jiff(jiff::Error),
    ExcessData,
    InternalInconsistency,
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

    pub fn ioctl_call(reason: &str) -> Self {
        Error::IoctlCall(format!("Calling ioctl had error: '{reason}'"))
    }
}

#[allow(clippy::from_over_into)] // Multiple implementations for .into() otherwise found
impl Into<Error> for evalexpr::EvalexprError {
    fn into(self) -> Error {
        Error::EvalExpr(self.to_string())
    }
}
