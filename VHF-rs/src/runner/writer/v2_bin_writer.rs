//! Writer method with newer methods.

use super::VHFWriter;
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

struct V2Writer {
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
    file_dir: PathBuf,
    current_file_handle: Arc<Mutex<Option<BufWriter<File>>>>,
    m_offset: u64,
}

impl VHFWriter for V2Writer {
    fn write_data(&mut self, words: super::WriteBlock) -> Result<()> {
        todo!()
    }

    fn close(&mut self) -> Result<()> {
        todo!()
    }
}

impl V2Writer {
    fn write_header(&self) {
        todo!()
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

impl Drop for V2Writer {
    fn drop(&mut self) {
        if thread::panicking() {
            log::warn!("V2Writer in panic. self = {:?}", self);
        }

        let tmp = self.close_file();
        if tmp.is_err() {
            log::warn!("Failed to close file during drop!");
        }
    }
}

impl Debug for V2Writer {
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
