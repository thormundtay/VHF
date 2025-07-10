//! This file uses the Python parsing through PyO3 for v1 file types. As such, it is dependent on
//! it being in the correct environment.

use super::{ParseError, ParseResult, RelTime, StartTime, VHFparse};
use pyo3::Bound;
use pyo3::prelude::Py;
use pyo3::prelude::PyAny;
use pyo3::prelude::PyAnyMethods;
use pyo3::prelude::PyModule;
use pyo3::prelude::PyResult;
use pyo3::prelude::Python;
use pyo3::types::IntoPyDict;
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
}

impl VHFparser {
    /// Get an instance of v1 VHF parser from Python.
    pub fn new<'b>(file: &'b Path, headers_only: bool) -> PyResult<Self> {
        let parser = Python::with_gil(|py| parser(py, file, headers_only))?;
        Ok(Self { parser })
    }
}

impl VHFparse for VHFparser {
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
        })
    }
}
