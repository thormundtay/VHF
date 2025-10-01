#[cfg(feature = "o3")]
use super::{ParseError, ParseResult};
#[cfg(feature = "o3")]
use pyo3::{Bound, IntoPyObject, PyAny, prelude::Python, types::PyAnyMethods};

use std::{cmp::Ordering, ops::Deref};

/// Absolute Time (Civil)
/// Work still to be done to check if timezone is required during parsing.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct AbsTime(pub jiff::Zoned);

impl AbsTime {
    #[cfg(feature = "o3")]
    pub(crate) fn into_pyobject<'py>(self, py: Python<'py>) -> ParseResult<Bound<'py, PyAny>> {
        self.0
            .into_pyobject(py)?
            .downcast::<PyAny>()
            .map_err(|e| ParseError::PyO3Downcast(e.to_string()))
            .cloned()
    }
}

impl From<jiff::Zoned> for AbsTime {
    fn from(value: jiff::Zoned) -> Self {
        AbsTime(value)
    }
}

impl Deref for AbsTime {
    type Target = jiff::Zoned;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Relative Time
#[derive(Clone, Debug)]
pub struct RelTime(pub jiff::Span);

impl RelTime {
    #[cfg(feature = "o3")]
    pub(crate) fn into_pyobject<'py>(self, py: Python<'py>) -> ParseResult<Bound<'py, PyAny>> {
        jiff::SignedDuration::try_from(self.0)
            .map_err(ParseError::JiffError)?
            .into_pyobject(py)?
            .downcast::<PyAny>()
            .map_err(|e| ParseError::PyO3Downcast(e.to_string()))
            .cloned()
    }
}

impl From<jiff::Span> for RelTime {
    fn from(value: jiff::Span) -> Self {
        Self(value)
    }
}

impl PartialEq for RelTime {
    fn eq(&self, other: &Self) -> bool {
        match self.0.compare(other.0) {
            Ok(Ordering::Less) => false,
            Ok(Ordering::Equal) => true,
            Ok(Ordering::Greater) => false,
            Err(_) => {
                log::warn!("RelTime comparison failed!");
                false
            }
        }
    }
}

impl Deref for RelTime {
    type Target = jiff::Span;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// For view window of Plot, one is free to specify either an absolute time, or time relative to
/// the file start.
#[derive(PartialEq, Clone, Debug)]
pub enum StartTime {
    /// This is the [jiff::Zoned] time for the start of the plot window.
    /// This will be coerced to be within the trace.
    Abs(AbsTime),
    /// This is the [jiff::Span] time relative to the file start for the plot window.
    /// This will be coerced to be within the trace.
    Rel(RelTime),
}

impl StartTime {
    #[cfg(feature = "o3")]
    pub(crate) fn into_pyobject<'py>(self, py: Python<'py>) -> ParseResult<Bound<'py, PyAny>> {
        match self {
            StartTime::Abs(v) => v.into_pyobject(py),
            StartTime::Rel(v) => v.into_pyobject(py),
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub enum DurationOrEndTime {
    /// Denotes the duration of the plot window, with the start time being being the last specified
    /// start time.
    /// This will be coerced to be within the trace.
    Rel(RelTime),
    /// Denotes the end of the plot window, with [jiff::Zoned] time.
    /// This will be coerced to be within the trace.
    Abs(AbsTime),
}

impl DurationOrEndTime {
    #[cfg(feature = "o3")]
    pub(crate) fn into_pyobject<'py>(self, py: Python<'py>) -> ParseResult<Bound<'py, PyAny>> {
        match self {
            DurationOrEndTime::Abs(v) => v.into_pyobject(py),
            DurationOrEndTime::Rel(v) => v.into_pyobject(py),
        }
    }
}
