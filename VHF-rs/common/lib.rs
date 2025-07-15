pub mod config_types;
pub mod data_types;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    ParseEmpty,
    ParseUnrecognised(String),
    Io(std::io::Error),
    Jiff(jiff::Error),
    InternalInconsistency,
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
