/// Methods for translating between Rust types to how some of our Python functions are written.
pub mod py_binds;
use py_binds::{RelTime, StartTime};

pub mod types;

/// Result associated to parsing of VHF file format.
pub type ParseResult<T> = core::result::Result<T, ParseError>;

use pyo3::PyErr;

/// Errors associated during parsing of VHF file format.
#[derive(Debug)]
pub enum ParseError {
    ValueError,
    PyError(PyErr),
    PyO3Downcast(String),
    JiffError(jiff::Error),
    InternalError,
}

impl From<PyErr> for ParseError {
    fn from(value: PyErr) -> Self {
        ParseError::PyError(value)
    }
}

pub mod v1_python;
pub use v1_python as v1;
