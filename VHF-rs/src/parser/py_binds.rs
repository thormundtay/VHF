use super::{ParseError, ParseResult};
use pyo3::prelude::Python;
use pyo3::types::PyAnyMethods;
use pyo3::{Bound, IntoPyObject, PyAny};
use std::cmp::Ordering;

/// Absolute Time (Civil)
/// Work still to be done to check if timezone is required during parsing.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct AbsTime(pub jiff::Zoned);

impl AbsTime {
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

/// Relative Time
#[derive(Clone, Debug)]
pub struct RelTime(pub jiff::Span);

impl RelTime {
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

/// For view window of Plot, one is free to specify either an absolute time, or time relative to
/// the file start.
pub enum StartTime {
    Abs(AbsTime),
    Rel(RelTime),
}

impl StartTime {
    pub(crate) fn into_pyobject<'py>(self, py: Python<'py>) -> ParseResult<Bound<'py, PyAny>> {
        match self {
            StartTime::Abs(v) => v.into_pyobject(py),
            StartTime::Rel(v) => v.into_pyobject(py),
        }
    }
}
