//! For the parsing of v2 file types.

mod rollover;
mod trace_timer;

use super::{DurationOrEndTime, StartTime};
use crate::{M, ParseError, ParseResult, VHFparse};
use byteorder::{NativeEndian, ReadBytesExt};
use jiff::{Span, Zoned};
use ndarray::{ArrayView, Ix1};
use rollover::RollOver;
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{BufReader, Read},
    path::Path,
    str::FromStr,
};
use trace_timer::TraceTimer;
use vhf_common::config_types::SamplingSpeed;
use vhf_common::magic::V2_MAGIC_HEADER;

/// Number of bytes up to and including bytes used to determine the rest of the header length.
// Magic Header + BOM + Magic Time + #Bytes of Header to read as u64
const PRE_REMAININGHEADER: usize = V2_MAGIC_HEADER.len() + 2 + 8 + 8;

/// Properties of trace as specified in header.
#[derive(Debug)]
pub struct TraceDetails {
    pub start_time: Zoned,
    /// This is the intended number of elements per file.
    pub num_samples: usize,
    /// Number of files that were requested to be created in the given run. Might not necessarily
    /// be true if the process was interrupted.
    pub num_files: usize,
    /// This value (known as "-s skipnum") is passed into the FPGA for decimation. Adding 1 to it
    pub skip_num: u16,
    /// This value (known as "-h" or "-l") is passed into the FPGA to operate at either 20 or 10
    /// MHz. Warns default as High if not specified.
    pub speed: SamplingSpeed,
    /// This is the dynamic gain (-g)
    pub gain: Option<u8>,
    /// This is the hardware filter (-F) used by the FPGA for low pass filtering.
    pub filter_const: Option<u8>,
    /// Runtime processing method, as declared by the method during the runtime.
    pub stream_fold: Vec<String>,
}

impl TraceDetails {
    fn new(s: &Value) -> ParseResult<Self> {
        let file_start = s
            .get("file_start")
            .ok_or_else(|| {
                log::error!("header['file_start'] not found");
                ParseError::ValueError
            })
            .and_then(|s| {
                log::debug!("Deserializing into Zoned = {}", &s);
                serde_json::from_value::<Zoned>(s.clone()).map_err(ParseError::SerdeJson)
            })?;

        let num_samples = s
            .get("num_samples")
            .ok_or_else(|| {
                log::error!("header['num_samples'] not found");
                ParseError::ValueError
            })
            .and_then(|n| {
                if n.is_u64() {
                    Ok(n.as_u64().unwrap() as _)
                } else {
                    log::error!("header['num_samples'] not found to be u64");
                    Err(ParseError::ValueError)
                }
            })?;

        let num_files = s
            .get("num_samples")
            .ok_or_else(|| {
                log::error!("header['num_files'] not found");
                ParseError::ValueError
            })
            .and_then(|n| {
                if n.is_u64() {
                    Ok(n.as_u64().unwrap() as _)
                } else {
                    log::error!("header['num_files'] not found to be u64");
                    Err(ParseError::ValueError)
                }
            })?;

        let skip_num = s
            .get("num_samples")
            .ok_or_else(|| {
                log::error!("header['num_samples'] not found");
                ParseError::ValueError
            })
            .and_then(|n| {
                if n.is_u64() {
                    Ok(n.as_u64().unwrap() as _)
                } else {
                    log::error!("header['num_samples'] not found to be u64-able");
                    Err(ParseError::ValueError)
                }
            })?;

        let speed = SamplingSpeed::from_str(
            s.get("speed")
                .and_then(|v| v.as_str()) // Option<&str>
                .unwrap_or_else(|| {
                    log::error!("header['speed'] had error, using default!");
                    // XXX: Hardcoded from enum name rather than FromStr method
                    "High"
                }),
        )
        .map_err(|e| {
            log::error!("Failed to SamplingSpeed::from_str with error = {e}");
            ParseError::ValueError
        })?;

        let gain: Option<u8> = match s.get("gain").unwrap_or(&Value::Null) {
            Value::Null => Ok(None),
            Value::Number(n) => n
                .as_u64()
                .ok_or_else(|| {
                    log::error!("gain was not u8");
                    ParseError::ValueError
                })
                .and_then(|v| {
                    if v > u8::MAX as u64 {
                        log::error!("could not convert gain `{v}` to u8");
                        Err(ParseError::ValueError)
                    } else {
                        Ok(Some(v as u8))
                    }
                }),
            e => {
                log::error!("unrecognised gain value: {e}");
                Err(ParseError::ValueError)
            }
        }?;

        let filter_const: Option<u8> = match s.get("filter_const").unwrap_or(&Value::Null) {
            Value::Null => Ok(None),
            Value::Number(n) => n
                .as_u64()
                .ok_or_else(|| {
                    log::error!("filter_const was not u8");
                    ParseError::ValueError
                })
                .and_then(|v| {
                    if v > u8::MAX as u64 {
                        log::error!("could not convert filter_const `{v}` to u8");
                        Err(ParseError::ValueError)
                    } else {
                        Ok(Some(v as u8))
                    }
                }),
            e => {
                log::error!("unrecognised filter_const value: {e}");
                Err(ParseError::ValueError)
            }
        }?;

        let stream_fold = {
            // This still probably needs more touch up depending on what filter can show up.
            s.get("stream_fold")
                .iter()
                .map(|&v| v.as_str().unwrap_or_default().to_string())
                .collect()
        };

        Ok(TraceDetails {
            start_time: file_start,
            num_samples,
            num_files,
            skip_num,
            speed,
            gain,
            filter_const,
            stream_fold,
        })
    }

    /// Determine the interval of time between 2 consecutive samples of the trace.
    pub fn sample_interval(&self) -> ParseResult<Span> {
        let base_ns = self.speed.in_ns();

        Ok(self.effective_decimation_factor() as i64 * base_ns)
    }

    /// Accounts skip-factor and software filters to determine the decimation factor relative to
    /// the FPGA sampling rate.
    pub fn effective_decimation_factor(&self) -> usize {
        // Until TraceDetails includes each filter's decimation factor, we assume it to be 1 for
        // now.
        self.skip_num as usize + 1
    }
}

/// Deconstruction of a v2 file.
// 'a: Lifetime of file (as path) being read from.
// 'd: Lifetime of mmap created during change of view window.
#[derive(Debug)]
pub struct VHFparser<'a, 'd>
where
    'a: 'd,
{
    file: &'a Path,
    /// This is the number of bytes associated header str.
    header_len: usize,
    /// This is the number of words associated to m_overflow indices.
    m_overflow_len: usize,
    header: Box<TraceDetails>,
    /// This is the number of words in the data section.
    data_len: usize,
    /// This is the offset block for the first m value.
    m_offset: M,
    /// Managing the plot window.
    timer: Box<TraceTimer>,
    /// This is the internal store of the view window.
    data: Option<ArrayView<'d, u64, Ix1>>, // No Rc<RefCell> due to passing out lifetime
}

impl<'a, 'd> VHFparser<'a, 'd> {
    /// V2 Binary file format parser.
    ///
    /// Data is only fetched when data or phase is requested.
    ///
    /// Arguments:
    /// - file: [Path] to file.
    /// - headers_only: If false, ManifoldManger is invoked not at init time, but at first data
    ///   fetch.
    pub fn new(file: &'a Path) -> ParseResult<Self> {
        log::debug!("Creating v2::VHFparser with {}", file.display());

        let mut file_bytes = BufReader::new(File::open(file)?).bytes();
        let iter = file_bytes.by_ref();

        // Reject if wrong magic.
        {
            let m_header: Vec<_> = iter.take(8).filter_map(Result::ok).collect();
            if str::from_utf8(&m_header)? != V2_MAGIC_HEADER {
                log::error!("File does not conform to V2 specification.");
                return Err(ParseError::ValueError);
            }
            log::debug!("Valid magic.");
        }

        // Reject if not NativeEndian.
        // While related to [bytemuck::Pod], it is easier to just use mmap.
        {
            let bom: Vec<_> = iter.take(2).filter_map(Result::ok).collect();
            if bom.as_slice() != target_bom() {
                log::error!("Reading file with different endianness from machine that wrote it!");
                log::error!("For optimization reasons, analysis of this is not done here in Rust.");
                return Err(ParseError::ValueError);
            }
            log::debug!("Same endianness as host.");
        }

        // Consume (collect required) next 8 bytes associated with approximal file start.
        iter.take(8).for_each(|_| {});

        // Determine rest of header
        let header_len: usize = {
            let mut tmp: Vec<u8> = Vec::with_capacity(8);
            iter.take(8).try_for_each(|v| -> ParseResult<()> {
                tmp.push(v?);
                Ok(())
            })?;
            tmp.as_slice().read_u64::<NativeEndian>()
        }? as _;
        log::trace!("header_len = {header_len}");

        let mut iter = iter.take(header_len);
        log::debug!("Trying to obtain header.");
        let mut header_str = Vec::new();
        iter.try_for_each(|r| r.map(|c| header_str.push(c)))?;
        let header_str = str::from_utf8(header_str.as_slice())?;
        log::debug!("header: str = {header_str}");
        let header_value = serde_json::from_str::<Value>(header_str)?;

        let header: Box<TraceDetails> = { Box::new(TraceDetails::new(&header_value)?) };

        let m_offset: M = header_value
            .get("m_offset")
            .ok_or_else(|| {
                log::error!("m_offset not found! This should not happen!");
                ParseError::ValueError
            })
            .and_then(|v| {
                v.as_i64().ok_or_else(|| {
                    log::error!("m_offset not i64! found: {v}");
                    ParseError::ValueError
                })
            })
            .and_then(|m| {
                m.try_into().map_err(|_| {
                    log::error!("m_offset read as i64 larger than internal M bounds.");
                    ParseError::InternalError
                })
            })?;
        let m_overflow_len = header_value
            .get("m_overflow_total")
            .ok_or_else(|| {
                log::error!("m_overflow_total not found! This should not happen!");
                ParseError::ValueError
            })
            .and_then(|v| {
                v.as_u64().ok_or_else(|| {
                    log::error!("m_overflow_total not u64! found: {v}");
                    ParseError::ValueError
                })
            })?
            .try_into()
            .map_err(|e| {
                log::error!("Could not convert m_overflow_total from u64 to usize: {e}");
                ParseError::ValueError
            })?;

        let file_len: usize = fs::metadata(file)
            .map(|f| f.len())?
            .try_into()
            .map_err(|e| {
                log::error!("Could not convert file_size from u64 to usize: {e}");
                ParseError::ValueError
            })?;
        let data_len_bytes =
            file_len - ((PRE_REMAININGHEADER + header_len).div_ceil(8) * 8) - (m_overflow_len * 8);
        if data_len_bytes % 8 != 0 {
            log::error!(
                "Was the file truncated whilst writing data? Data length is not multiple of 8 bytes."
            );
            return Err(ParseError::ValueError);
        }
        let data_len = data_len_bytes / 8;

        let timer = {
            let trace_start = header.start_time.clone();
            let sample_interval = TraceDetails::sample_interval(&header)?;
            let trace_len = data_len;

            Box::new(TraceTimer::new(trace_start, sample_interval, trace_len)?)
        };

        {
            // Warn if the declared length is not the same as what is recorded.
            if data_len != header.num_samples {
                log::warn!(
                    "{} declared num_samples = {}, found {data_len}",
                    file.display(),
                    header.num_samples
                );
            }
        }

        Ok(Self {
            file,
            header_len,
            m_overflow_len,
            header,
            data_len,
            m_offset,
            timer,
            data: None,
        })
    }
}

impl<'a, 'd> VHFparse<'d> for VHFparser<'a, 'd>
where
    'a: 'd,
{
    type DataReturn = ArrayView<'d, u64, Ix1>;
    type TransformReturn<T: 'd> = ArrayView<'d, T, Ix1>;

    fn update_plot_timing(
        &mut self,
        start: Option<StartTime>,
        duration_or_end: Option<DurationOrEndTime>,
        lazy: bool,
    ) -> ParseResult<()> {
        todo!()
    }

    fn data(&self) -> ParseResult<Self::DataReturn> {
        if self.data.is_some() {
            return Ok(*self.data.as_ref().unwrap());
        }
        todo!()
    }

    fn resolve_m_overflow_idxs(&mut self) -> ParseResult<()> {
        todo!()
    }

    fn reduced_phase(&self) -> ParseResult<Self::TransformReturn<crate::ReducedPhase>> {
        todo!()
    }
}

/// Determine the BOM associated with the target compiled against.
#[inline(always)]
const fn target_bom() -> [u8; 2] {
    #[cfg(target_endian = "little")]
    {
        [0xFF, 0xFE]
    }
    #[cfg(target_endian = "big")]
    {
        [0xFE, 0xFF]
    }
}
