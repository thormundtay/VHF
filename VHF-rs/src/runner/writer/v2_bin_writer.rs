//! Writer method with newer methods.

use super::{FILE_LAZY_LEN, VHFWriter};
use crate::{Error, Result, types::RawVHFWord};
#[cfg(not(test))]
use jiff::SignedDuration;
use jiff::{Span, Zoned};
use std::{
    fmt::Debug,
    fs::{File, OpenOptions},
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

pub(super) const V2_MAGIC_HEADER: &str = "VHFV2BIN";

pub struct V2BinWriter {
    /// Timestamp of the first file's first datapoint.
    start_time: Zoned,
    /// Total number of files expected.
    num_files: usize,
    /// Number of files that have been opened so far.
    num_files_so_far: AtomicUsize,
    time_between_files: Span,
    /// This is the number of [crate::runner::Config::num_samples] to be eventually be written to file.
    num_elements_per_file: usize,
    num_elements_written: AtomicUsize,
    verbosity: u8,
    header_details: String,
    filename_details: String,
    /// Avoid writing to a file until we exceed some amount.
    elements_to_write: Arc<Mutex<Vec<RawVHFWord>>>,
    m_overflow_to_write: Arc<Mutex<Vec<i8>>>,
    file_dir: PathBuf,
    current_file_handle: Arc<Mutex<Option<BufWriter<File>>>>,
    /// This is the size of the header in bytes before the start of m_overflow block.
    header_bytes: usize,
    /// This is the number of elements seen so far. This is necessary for ensuring that m_overflow
    /// idx is correct. This differs from num_elements_written as this may be nonzero while nothing
    /// has yet been written.
    num_elements_seen: AtomicUsize,
    /// This is the overflow of i16 associated to `M` of the first data point.
    /// As such, m_offset = +1 denotes that the first data point have
    /// (phase / 2pi) = arctan(Q/I)/2pi + m + (m_offset * u16::MAX).
    m_offset: i64,
    /// This is the maximum allowed number of m_overflow elements allowed to be write.
    /// We are allowed to overwrite the magic value.
    m_overflow_total: usize,
    /// This is the number of m_overflow elements written so far.
    m_overflow_written: usize,
}

impl VHFWriter for V2BinWriter {
    fn write_data(&mut self, words: super::WriteBlock) -> Result<()> {
        todo!()
    }

    fn close(&mut self) -> Result<()> {
        todo!()
    }
}

impl V2BinWriter {
    /// Creates an object that allows for writing of data processed out of
    /// [crate::runner::process::VHF]. Whilst still working out if [crate::runner::Config] contains
    /// enough information about the runtime, the `main()` function should instead be responsible
    /// for determining the time by which the first data point is being written to file. This means
    /// that data points being dropped in processing should be accounted for.
    fn new(config: V2BinArg, start_time: jiff::Zoned) -> Self {
        Self {
            start_time,
            num_files: *config.num_files,
            num_files_so_far: AtomicUsize::new(0),
            time_between_files: *config.file_timespan,
            num_elements_per_file: *config.num_samples,
            num_elements_written: AtomicUsize::new(0),
            verbosity: *config.verbosity,
            header_details: config.header_details.to_string(),
            filename_details: config.filename_details.to_string(),
            elements_to_write: Arc::new(Mutex::new(Vec::with_capacity(
                FILE_LAZY_LEN.min(*config.num_samples),
            ))),
            m_overflow_to_write: Arc::new(Mutex::new(Vec::with_capacity(
                FILE_LAZY_LEN.min(*config.num_samples),
            ))),
            file_dir: config.save_dir.to_path_buf(),
            current_file_handle: Arc::new(Mutex::new(None)),
            header_bytes: 0,
            num_elements_seen: AtomicUsize::new(0),
            m_offset: 0,
            m_overflow_total: *config.m_overflow_total,
            m_overflow_written: 0,
        }
    }

    /// Tries to open a file in the specified location with the required name. Fails if file
    /// already exists.
    fn open_file(&mut self) -> Result<BufWriter<File>> {
        todo!()
    }

    fn write_header(&self) -> Result<()> {
        // Do not write into a file who has already had headers/data written.
        if self.current_file_handle.lock().unwrap().is_none()
            || self.num_elements_written.load(Ordering::Acquire) > 0
        {
            return Err(Error::InternalInconsistency);
        }

        let file_start_time = self
            .start_time
            .checked_add(
                self.time_between_files
                    // Because the file has already been opened, we have to sub by 1.
                    * self.num_files_so_far
                        .load(Ordering::Acquire)
                        .checked_sub(1)
                        .unwrap() as i64,
            )
            .map_err(Error::Jiff)?;
        let file_start_unix: jiff::Timestamp = file_start_time.into();

        use byteorder::{NativeEndian, WriteBytesExt};
        use std::io::Write;
        if let Some(buf_file) = self.current_file_handle.lock().unwrap().as_mut() {
            buf_file
                .write(V2_MAGIC_HEADER.as_bytes())
                .map_err(Error::Io)?; // Write magic in UTF.
            buf_file
                .write_u16::<NativeEndian>(0xFEFF)
                .map_err(Error::Io)?; // Write BOM.
            buf_file
                .write_i64::<NativeEndian>(file_start_unix.as_second())
                .map_err(Error::Io)?; // Write UNIX time stamp.
        }

        Ok(())
    }

    fn close_file(&mut self) -> Result<()> {
        if let Some(file) = self.current_file_handle.lock().unwrap().as_mut() {
            use std::io::Write;
            file.flush().map_err(Error::Io)?;
        }
        *self.current_file_handle.lock().unwrap() = None;
        Ok(())
    }
}

impl Drop for V2BinWriter {
    fn drop(&mut self) {
        if thread::panicking() {
            log::warn!("V2Writer in panic. self = {self:?}");
        }

        let tmp = self.close_file();
        if tmp.is_err() {
            log::warn!("Failed to close file during drop!");
        }
    }
}

impl Debug for V2BinWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"files", &self.num_files_so_far)
            .entry(&"files_tot", &self.num_files)
            .entry(&"elements_written", &self.num_elements_written)
            .entry(&"elements_tot", &self.num_elements_per_file)
            .entry(
                &"internal_buf_len",
                &self.elements_to_write.try_lock().map(|x| (*x).len()),
            )
            .entry(&"file_dir", &self.file_dir)
            .entry(&"m_offset", &self.m_offset)
            // Don't really care for the internal headers etc as they are const
            .finish()
    }
}

#[derive(Debug, Clone)]
pub(super) struct V2BinArg<'a> {
    pub num_samples: &'a usize,
    pub num_files: &'a usize,
    pub verbosity: &'a u8,
    pub file_timespan: Box<Span>,
    pub header_details: String,
    pub filename_details: String,
    pub save_dir: &'a Path,
    pub m_overflow_total: &'a usize,
}
