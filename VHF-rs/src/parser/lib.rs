mod consts;

/// Methods for translating between Rust types to how some of our Python functions are written.
pub mod py_binds;
use py_binds::{DurationOrEndTime, StartTime};

/// Result associated to parsing of VHF file format.
pub type ParseResult<T> = core::result::Result<T, ParseError>;

#[cfg(feature = "o3")]
use pyo3::PyErr;

/// Errors associated during parsing of VHF file format.
#[derive(Debug)]
pub enum ParseError {
    Excess,
    ValueError,
    #[cfg(feature = "o3")]
    PyError(PyErr),
    PyO3Downcast(String),
    #[cfg(feature = "o3")]
    BorrowError(numpy::BorrowError),
    JiffError(jiff::Error),
    IoError(std::io::Error),
    ByteMuckError(bytemuck::PodCastError),
    Utf8Error,
    SerdeJson(serde_json::Error),
    ByteMuckCastError(bytemuck::checked::CheckedCastError),
    InternalError,
}

#[cfg(feature = "o3")]
impl From<PyErr> for ParseError {
    fn from(value: PyErr) -> Self {
        ParseError::PyError(value)
    }
}

impl From<std::io::Error> for ParseError {
    fn from(value: std::io::Error) -> Self {
        ParseError::IoError(value)
    }
}

impl From<std::str::Utf8Error> for ParseError {
    fn from(_: std::str::Utf8Error) -> Self {
        ParseError::Utf8Error
    }
}

impl From<serde_json::Error> for ParseError {
    fn from(value: serde_json::Error) -> Self {
        ParseError::SerdeJson(value)
    }
}

impl From<bytemuck::PodCastError> for ParseError {
    fn from(value: bytemuck::PodCastError) -> Self {
        ParseError::ByteMuckError(value)
    }
}

impl From<bytemuck::checked::CheckedCastError> for ParseError {
    fn from(value: bytemuck::checked::CheckedCastError) -> Self {
        ParseError::ByteMuckCastError(value)
    }
}

// Expectation for Data Types returned by parse methods.
/// Raw VHF word prior to any parsing.
pub use vhf_common::data_types::RawVHFWord as VHFWord;
pub use vhf_common::data_types::VHFWordT as WordT;
/// M component of [VHFWord].
type M = i32;
/// Unwrapped reduced phase of [VHFWord].
pub type ReducedPhase = f64;

use ndarray::Array1;
use std::marker::PhantomData;

/// Data/TransformedView within file as constrained by temporal bounds set in [VHFparse::update_plot_timing].
///
/// This can be thought of as a [Array1] but with lifetime bounded to function returning this type.
/// Consume this struct through the `take` method.
// Note: Deliberately not implementing Clone currently, as this requires cloning all elements
// within the Array1 currently.
#[derive(Debug)]
pub struct DataView<'d, T> {
    pub data: Array1<T>,
    _lifetime: PhantomData<&'d T>,
}

impl<'d, T> std::ops::Deref for DataView<'d, T> {
    type Target = Array1<T>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T> DataView<'_, T> {
    /// This is a convenience method so that cloning arbitrarily shouldn't be a thing.
    pub fn take(self) -> Array1<T> {
        self.data
    }
}

/// Expected methods of any VHF parser. Mirrors Python's expectations.
pub trait VHFparse {
    /// Return type of `data` method.
    ///
    /// May vary by implementation.
    // v1 Writer unfortunately has to return as an Owned Array by the current PyO3 bindings, but it
    // is quite likely that v2 will return as a View Array.
    type Data<'d>
    where
        Self: 'd;

    /// Return type of `reduced phase` method.
    ///
    /// May vary by implementation.
    // Note: In the context of v2 parsing, note that ArrayView + ArrayView is not allowed. As such,
    // it is also likely that this has a similar issue to needing [DataView].
    type ReducedPhase<'d>
    where
        Self: 'd;

    /// Update the class to be aware of all m-overflow indices. This is in the event that the
    /// parser tries to be lazy at init time.
    fn resolve_m_overflow_idxs(&mut self) -> ParseResult<()>;

    /// Changes the expected view window associated with the current parsed file.
    ///
    /// This function differs from the Python implementation where the start and end time is
    /// instead to be specified rather than having a flexible combination.
    ///
    /// # Arguments
    /// - Lazy: Defer data fetch from when this function is called.
    fn update_plot_timing(
        &mut self,
        start: Option<StartTime>,
        duration_or_end: Option<DurationOrEndTime>,
        lazy: bool,
    ) -> ParseResult<()>;

    /// Block of binary trace in accordance with plot window specified.
    fn data<'a>(&'a self) -> ParseResult<Self::Data<'a>>;

    /// Block of binary trace with unwrapped phase / 2pi.
    fn reduced_phase<'a>(&'a self) -> ParseResult<Self::ReducedPhase<'a>>;
}

pub mod unwrap_phase;

#[cfg(feature = "o3")]
pub mod v1_python;
#[cfg(feature = "o3")]
pub use v1_python as v1;

pub mod v2;
