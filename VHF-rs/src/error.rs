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
    User,
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
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

impl From<evalexpr::EvalexprError> for Error {
    fn from(value: evalexpr::EvalexprError) -> Self {
        Error::EvalExpr(value.to_string())
    }
}

impl From<vhf_common::Error> for Error {
    fn from(value: vhf_common::Error) -> Self {
        match value {
            vhf_common::Error::InternalInconsistency => Self::InternalInconsistency,
            vhf_common::Error::Io(v) => Self::Io(v),
            vhf_common::Error::Jiff(v) => Self::Jiff(v),
            vhf_common::Error::ParseEmpty => Self::ParseEmpty,
            vhf_common::Error::ParseUnrecognised(v) => Self::ParseUnrecognised(v),
            vhf_common::Error::ExcessData => Self::ExcessData,
        }
    }
}
