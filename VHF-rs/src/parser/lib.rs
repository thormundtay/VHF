/// Methods for translating between Rust types to how some of our Python functions are written.
pub mod py_binds;
use py_binds::{AbsTime, RelTime, StartTime};

/// Result associated to parsing of VHF file format.
pub type ParseResult<T> = core::result::Result<T, ParseError>;

use pyo3::PyErr;

/// Errors associated during parsing of VHF file format.
#[derive(Debug)]
pub enum ParseError {
    ValueError,
    PyError(PyErr),
    PyO3Downcast(String),
    BorrowError(numpy::BorrowError),
    JiffError(jiff::Error),
    InternalError,
}

impl From<PyErr> for ParseError {
    fn from(value: PyErr) -> Self {
        ParseError::PyError(value)
    }
}

// Expectation for Data Types returned by parse methods.
/// Raw VHF word prior to any parsing.
pub use vhf_common::data_types::RawVHFWord as VHFWord;
/// Unwrapped reduced phase of [VHFWord].
type ReducedPhase = f64;

/// Expected methods of any VHF parser. Mirrors Python's expectations.
pub trait VHFparse {
    /// v1 Writer unfortunately has to return as an Owned Array, but it is quite likely that v2
    /// will return as a View Array.
    type DataReturn;

    // /// This changes in accordance with if Map is collected, or if taken from Python etc.
    type TransformReturn<T>;

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

    /// Block of binary trace in accordance with plot window specified.
    fn data(&self) -> ParseResult<Self::DataReturn>;

    /// Block of binary trace with unwrapped phase / 2pi.
    fn reduced_phase(&self) -> ParseResult<Self::TransformReturn<ReducedPhase>>;
}

pub mod v1_python;
pub use v1_python as v1;
