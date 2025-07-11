//! This file uses the Python parsing through PyO3 for v1 file types. As such, it is dependent on
//! it being in the correct environment.

use super::{AbsTime, ParseError, ParseResult, RelTime, StartTime, VHFWord, VHFparse};
use ndarray::Array1;
use numpy::PyArray1;
use pyo3::Bound;
use pyo3::prelude::Py;
use pyo3::prelude::PyAny;
use pyo3::prelude::PyAnyMethods;
use pyo3::prelude::PyModule;
use pyo3::prelude::PyResult;
use pyo3::prelude::Python;
use pyo3::types::IntoPyDict;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

/// Gets instantised v1_parser class.
fn parser<'py, 'b>(py: Python<'py>, file: &'b Path, headers_only: bool) -> PyResult<Py<PyAny>> {
    let v1_mod = PyModule::import(py, "VHF._parse.v1")?; // Fails if PYTHONPATH is wrong

    let p = v1_mod.getattr("VHFparser")?;
    let kwargs: HashMap<&str, &str> = {
        let mut hm = HashMap::new();
        hm.entry("headers_only").or_insert(match headers_only {
            false => "False",
            true => "True",
        });
        hm
    };
    let kwargs = kwargs.into_py_dict(py)?;

    let unbind = p.call((file,), Some(&kwargs))?.unbind();
    Ok(unbind)
}

/// Rust representation of Python v1 parser.
pub struct VHFparser {
    parser: Py<PyAny>,
    /// Rust specified user previous start, for managing [self::data_rs].
    start: Box<Option<AbsTime>>,
    /// Rust specified user previous duration, for managing [self::data_rs].
    duration: Box<Option<RelTime>>,
    /// Rust owned reflection of Python's data for GIL reasons.
    data_rs: RefCell<Option<Array1<VHFWord>>>,
}

impl VHFparser {
    /// Get an instance of v1 VHF parser from Python.
    pub fn new<'b>(file: &'b Path, headers_only: bool) -> PyResult<Self> {
        let parser = Python::with_gil(|py| parser(py, file, headers_only))?;
        let start = Box::new(None);
        let duration = Box::new(None);
        let data_rs = RefCell::new(None);

        Ok(Self {
            parser,
            start,
            duration,
            data_rs,
        })
    }
}

impl VHFparse for VHFparser {
    type DataReturn = Array1<VHFWord>;

    fn resolve_m_overflow_idxs(&self) -> ParseResult<()> {
        Python::with_gil(|py| -> PyResult<()> {
            self.parser
                .bind(py)
                .call_method("resolve_m_overflow_idxs", (), None)?;

            Ok(())
        })?;

        Ok(())
    }

    fn update_plot_timing(
        &mut self,
        start: StartTime,
        duration: RelTime,
        lazy: bool,
    ) -> ParseResult<()> {
        let start_abs: AbsTime = todo!();

        // If input was not changed, early exit.
        if self.start.is_some_and(|s| s == start_abs)
            && self.duration.is_some_and(|d| d == duration)
        {
            return Ok(());
        }

        // Otherwise, invalidate and call into Python.
        self.start = Box::new(Some(start_abs));
        self.duration = Box::new(Some(duration));
        *self.data_rs.borrow_mut() = None;

        Python::with_gil(|py| -> ParseResult<()> {
            let kwargs = {
                let kv: [(&str, Bound<PyAny>); 2] = [
                    ("start", start.into_pyobject(py)?),
                    ("duration", duration.into_pyobject(py)?),
                ];
                kv.into_py_dict(py)?
            };

            self.parser.bind(py).call_method(
                "update_plot_timing",
                (match lazy {
                    false => "False",
                    true => "True",
                },),
                Some(&kwargs),
            )?;
            Ok(())
        })?;

        if !lazy {
            self.data()?;
        }

        Ok(())
    }

    /// Provisions owned Data as provided by VHF(py) parser class.
    ///
    /// WARN: To preserve the idiomatic Rust code, data has to be allocated on to the Rust heap
    /// on top of the Python heap.
    fn data<'a>(&'a self) -> ParseResult<Self::DataReturn> {
        // Fetch from Python.
        if self.data_rs.borrow().is_none() {
            let array = Python::with_gil(|py| -> ParseResult<Array1<VHFWord>> {
                let result = self.parser.bind(py).getattr("data")?;
                let result = result
                    .downcast::<PyArray1<VHFWord>>()
                    .map_err(|e| ParseError::PyO3Downcast(e.to_string()))?;

                use numpy::PyArrayMethods;
                let readonly = result.try_readonly().map_err(ParseError::BorrowError)?;
                let array = readonly.as_array().to_owned(); // Clone to outlive GIL

                Ok(array)
            })?;
            *self.data_rs.borrow_mut() = Some(array);
        }

        let arr = self
            .data_rs
            .borrow()
            .clone()
            .ok_or(ParseError::InternalError);
        arr
    }
}
