//! This file uses the Python parsing through PyO3 for v1 file types. As such, it is dependent on
//! it being in the correct environment.

use super::{DurationOrEndTime, ParseError, ParseResult, StartTime, VHFWord, VHFparse};
use crate::ReducedPhase;
use crate::py_binds::AbsTime;
use jiff::Zoned;
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
use pyo3::types::PyBytes;
use pyo3::types::PyDateTime;
use pyo3::types::PyDict;
use pyo3::types::PyInt;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::num::NonZeroU64;
use std::path::Path;
use vhf_common::config_types::SamplingSpeed;

/// For VHF package to check if inited properly.
#[cfg(feature = "o3")]
pub fn test_import() -> bool {
    Python::with_gil(|py| PyModule::import(py, "VHF").is_ok())
}

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
    header: Py<PyDict>,
    headerraw: Vec<u8>,
    file_start: Box<Zoned>,
    /// Rust specified user previous start, for managing [self::data_rs].
    start: Box<Option<AbsTime>>,
    /// Rust specified user previous duration, for managing [self::data_rs].
    duration_or_end: Box<Option<DurationOrEndTime>>,
    /// Rust owned reflection of Python's data for GIL reasons.
    data_rs: RefCell<Option<Array1<VHFWord>>>,
    /// Rust owned reflection of Python's data for GIL reasons.
    phase_rs: RefCell<Option<Array1<ReducedPhase>>>,
}

impl VHFparser {
    /// Get an instance of v1 VHF parser from Python.
    pub fn new<'b>(file: &'b Path, headers_only: bool) -> ParseResult<Self> {
        let parser = Python::with_gil(|py| parser(py, file, headers_only))?;
        // start = VHFparser(file).header["Time start"]  # <class 'datetime.datetime'> -> PyDateTime
        let file_start = Box::new(Python::with_gil(|py| -> PyResult<Zoned> {
            let header: Bound<'_, PyDict> = parser
                .bind(py)
                .getattr("header")?
                .downcast_into::<PyDict>()?;

            use pyo3::types::PyDictMethods;
            let val: Bound<'_, PyDateTime> = header
                .get_item("Time start")?
                .expect("No 'Time start' found in keys of header")
                .downcast_into::<PyDateTime>()?;

            let unb: Py<PyDateTime> = val.unbind();
            let e: Zoned = unb.extract(py)?;
            Ok(e)
        })?);
        let start = Box::new(None);
        let duration_or_end = Box::new(None);
        let data_rs = RefCell::new(None);
        let phase_rs = RefCell::new(None);
        let (headerraw, header) = Self::fetch_header(&parser)?;

        let s = Self {
            parser,
            header,
            headerraw,
            file_start,
            start,
            duration_or_end,
            data_rs,
            phase_rs,
        };
        if !headers_only {
            s.data()?;
        }

        Ok(s)
    }

    /// Get parser.header and parser.headerraw
    fn fetch_header(parser: &Py<PyAny>) -> ParseResult<(Vec<u8>, Py<PyDict>)> {
        Python::with_gil(|py| -> ParseResult<_> {
            let h_str = parser
                .getattr(py, "headerraw")
                .map_err(ParseError::PyError)?
                .downcast_bound::<PyBytes>(py)
                .map_err(|e| ParseError::PyO3Downcast(e.to_string()))?
                .clone()
                .unbind()
                .as_bytes(py)
                .to_vec();
            let dict = parser
                .getattr(py, "header")
                .map_err(ParseError::PyError)?
                .downcast_bound::<PyDict>(py)
                .map_err(|e| ParseError::PyO3Downcast(e.to_string()))?
                .clone()
                .unbind();

            Ok((h_str, dict))
        })
    }

    /// Gets the header "dictionary" associated with the file.
    pub fn header(&self) -> VHFheader<'_> {
        VHFheader::new(&self.headerraw, &self.header)
    }
}

impl VHFparse for VHFparser {
    type DataReturn = Array1<VHFWord>;
    type TransformReturn<T> = Array1<T>;

    fn resolve_m_overflow_idxs(&mut self) -> ParseResult<()> {
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
        start: Option<StartTime>,
        duration_or_end: Option<DurationOrEndTime>,
        lazy: bool,
    ) -> ParseResult<()> {
        // We don't attempt to maintain a true mirror of the Python state in this Rust wrapper at
        // the moment as to what the plot_start and plot_end is.
        let start_abs: Option<AbsTime> = {
            // Not using Option::map due to closure escape
            let arg_start = if let Some(s) = start.clone() {
                match s {
                    StartTime::Abs(ref v) => Some(v.clone()),
                    StartTime::Rel(ref r) => Some(
                        (*self.file_start)
                            .checked_add(r.0)
                            .map_err(ParseError::JiffError)?
                            .into(),
                    ),
                }
            } else {
                None
            };

            if arg_start.is_none() {
                *self.start.clone()
            } else {
                arg_start
            }
        };

        // If input was not changed, early exit.
        if (*self.start == start_abs) && (*self.duration_or_end == duration_or_end) {
            return Ok(());
        }

        // Otherwise, invalidate and call into Python.
        self.start = Box::new(start_abs);
        self.duration_or_end = Box::new(duration_or_end.clone());
        *self.data_rs.borrow_mut() = None;
        *self.phase_rs.borrow_mut() = None;

        Python::with_gil(|py| -> ParseResult<()> {
            let kwargs = {
                let mut kv: Vec<(&str, Bound<PyAny>)> = Vec::with_capacity(2);
                if let Some(s) = start {
                    kv.push(("start", s.into_pyobject(py)?));
                };
                if let Some(d) = duration_or_end {
                    kv.push(("duration", d.into_pyobject(py)?));
                };
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
    fn data(&self) -> ParseResult<Self::DataReturn> {
        // Fetch from Python.
        if self.data_rs.borrow().is_none() {
            let array = Python::with_gil(|py| -> ParseResult<Array1<u64>> {
                let result = self.parser.bind(py).getattr("data")?;
                let result = result
                    .downcast::<PyArray1<u64>>()
                    .map_err(|e| ParseError::PyO3Downcast(e.to_string()))?;

                use numpy::PyArrayMethods;
                let readonly = result.try_readonly().map_err(ParseError::BorrowError)?;
                let array = readonly.as_array().to_owned(); // Clone to outlive GIL

                Ok(array)
            })?
            .mapv(VHFWord::from);

            *self.data_rs.borrow_mut() = Some(array);
        }

        self.data_rs
            .borrow()
            .clone()
            .ok_or(ParseError::InternalError)
    }

    /// For now, fetches from Python; Will chang to using Rust Mapv.
    fn reduced_phase(&self) -> ParseResult<Self::TransformReturn<ReducedPhase>> {
        // Fetch from Python.
        if self.phase_rs.borrow().is_none() {
            let array = Python::with_gil(|py| -> ParseResult<Array1<ReducedPhase>> {
                let result = self.parser.bind(py).getattr("reduced_phase")?;
                let result = result
                    .downcast::<PyArray1<ReducedPhase>>()
                    .map_err(|e| ParseError::PyO3Downcast(e.to_string()))?;

                use numpy::PyArrayMethods;
                let readonly = result.try_readonly().map_err(ParseError::BorrowError)?;
                let array = readonly.as_array().to_owned(); // Clone to outlive GIL

                Ok(array)
            })?;
            *self.phase_rs.borrow_mut() = Some(array);
        }

        self.phase_rs
            .borrow()
            .clone()
            .ok_or(ParseError::InternalError)
    }
}

impl Debug for VHFparser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V1_VHFparser")
            .field("parser", &self.parser)
            .field("file_start", &self.file_start)
            .field("start", &self.start)
            .field("duration_or_end", &self.duration_or_end)
            .field(
                "data_rs.len()",
                &self.data_rs.borrow().clone().map(|l| l.len()).unwrap_or(0),
            )
            .finish()
    }
}

pub struct VHFheader<'a> {
    pub header_raw: &'a [u8],
    pub header_dict: &'a Py<PyDict>,
    pub verbosity: u8,
    /// Base sampling speed. None indicates it was not set. Implicitly, None should be treated as
    /// high frequency.
    pub speed: Option<SamplingSpeed>,
    pub start_time: Option<Box<Zoned>>,
    skip_factor: Option<u16>,
}

impl<'a> VHFheader<'a> {
    fn new(header_str: &'a [u8], header_dict: &'a Py<PyDict>) -> Self {
        let (verbosity, speed, start_time, skip_factor) = Python::with_gil(|py| {
            let dict = header_dict.bind(py);

            let v: u8 = match dict.get_item("v") {
                Ok(d) => d
                    .downcast_into::<PyInt>()
                    .ok()
                    .and_then(|p| p.extract().ok())
                    .unwrap_or(0),
                Err(_) => 0,
            };
            let speed = match v {
                0 => None,
                _ => {
                    if dict.get_item("l").is_ok() {
                        Some(SamplingSpeed::Low)
                    } else if dict.get_item("h").is_ok() {
                        Some(SamplingSpeed::High)
                    } else {
                        None
                    }
                }
            };
            let start: Option<Zoned> = match v {
                0 => None,
                _ => {
                    let t = dict.get_item("Time start");
                    log::trace!("Time start = {:?}", t);
                    t.ok().and_then(|t| t.extract().ok())
                }
            };
            let skip: Option<u16> = match v {
                0 => None,
                _ => dict.get_item("s").ok().and_then(|p| p.extract().ok()),
            };

            (v, speed, start.map(Box::new), skip)
        });

        Self {
            header_raw: header_str,
            header_dict,
            verbosity,
            speed,
            start_time,
            skip_factor,
        }
    }

    /// This gives the effective skip factor after taking into account software filters. This is
    /// used for determining the effective frequency of the data written to file.
    ///
    /// Note: V1 files should not contain any software filters.
    pub fn effective_decimation_factor(&'a self) -> NonZeroU64 {
        log::trace!("skip_factor = {:?}", self.skip_factor);
        unsafe {
            NonZeroU64::new(self.skip_factor.unwrap_or(0u16) as u64 + 1u64).unwrap_unchecked()
        }
    }
}
