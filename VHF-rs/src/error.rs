use core::error;
use std::fmt;

pub type Result<T> = core::result::Result<T, Error>;

// This should move into an enum as the error types are made clearer
#[derive(Debug)]
pub struct Error {
    details: String,
}

impl Error {
    pub fn new(details: &str) -> Error {
        Error {
            details: details.to_string(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmtter: &mut fmt::Formatter) -> fmt::Result {
        write!(fmtter, "{}", self.details)
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        &self.details
    }
}
