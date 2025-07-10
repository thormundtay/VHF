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

/// Expected methods of any VHF parser. Mirrors Python's expectations.
pub trait VHFparse {
    /// Update the class to be aware of all m-overflow indices. This is in the event that the
    /// parser tries to be lazy at init time.
    fn resolve_m_overflow_idxs(&self) -> ParseResult<()>;

    /// Changes the expected view window associated with the current parsed file.
    ///
    /// This function differs from the Python implementation where the start and end time is
    /// instead to be specified rather than having a flexible combination.
    ///
    /// # Arguments
    /// - Lazy: Defer data fetch from when this function is called.
    fn update_plot_timing(
        &mut self,
        start: StartTime,
        duration: RelTime,
        lazy: bool,
    ) -> ParseResult<()>;
}

pub mod v1_python;
pub use v1_python as v1;
