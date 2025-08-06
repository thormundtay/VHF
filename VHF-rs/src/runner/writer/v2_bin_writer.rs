//! V2Writer is an improved file-format writer that follows the V2 spec.
//!
//! The V2 spec aims to resolve several problems involved with throughput of file reading,
//! primarily due to having to check the entire file at first for any m_overflow phenomenon, before
//! being able to read any relevant block of traces.
//! This is done so by recording the location of m-overflow after the header of the file, but
//! before the data section of the file, known as `m_overflow_idx` or similar to describe this
//! section of data. For more information, see [MOverflowRaw].

use super::super::BoardConfig;
use super::{FILE_LAZY_LEN, VHFWriter};
use crate::{Error, Result};
#[cfg(not(test))]
use jiff::SignedDuration;
use jiff::{Span, Zoned};
use serde::Serialize;
use std::collections::VecDeque;
use std::{
    fmt::Debug,
    fs::{File, OpenOptions},
    io::BufWriter,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};
use vhf_common::data_types::{MOverflowRaw, RawVHFWord};
use vhf_common::magic::V2_MAGIC_HEADER;
use vhf_common::write_types::MOverflowWrite;

pub struct V2BinWriter<'a> {
    /// Timestamp of the board's start time.
    start_time: Box<Zoned>,
    /// Construct the header associated with the file.
    board_config: Box<BoardConfig<'a>>,
    /// Total number of files expected.
    num_files: usize,
    /// Number of files that have been opened so far.
    num_files_so_far: AtomicUsize,
    time_between_files: Box<Span>,
    /// This is the number of [crate::runner::Config::num_samples] to be eventually be written to file.
    num_elements_per_file: usize,
    /// This is the number of elements written to the current file.
    num_elements_written: AtomicUsize,
    verbosity: u8,
    filename_details: String,
    /// Avoid writing to a file until we exceed some amount.
    elements_to_write: Arc<Mutex<Vec<RawVHFWord>>>,
    m_overflow_to_write: Arc<Mutex<VecDeque<MOverflowRaw>>>,
    file_dir: PathBuf,
    current_file_handle: Arc<Mutex<Option<BufWriter<File>>>>,
    /// This is the number of bytes (rounded up to the nearest word) that is dedicated to header.
    header_len: Option<NonZeroUsize>,
    /// This is the overflow of i16 associated to `M` of the first data point.
    /// As such, m_offset = +1 denotes that the first data point have
    /// (phase / 2pi) = arctan(Q/I)/2pi + m + (m_offset * u16::MAX).
    m_offset: i64,
    /// This is the maximum allowed number of m_overflow elements allowed to be write.
    /// We are allowed to overwrite the magic value.
    m_overflow_total: usize,
    /// This is the number of m_overflow elements written so far.
    m_overflow_written: AtomicUsize,
}

impl<'a> VHFWriter for V2BinWriter<'a> {
    fn write_data(&mut self, words: super::WriteBlock) -> Result<()> {
        // Clone needed because [WriteBlock::overflow] is consuming
        let data: &mut Vec<_> = &mut words.data.clone();

        self.push_onto_m_overflow_to_write(words.overflow())?;

        'data_has_element: loop {
            if self.num_files_so_far.load(Ordering::Acquire) > self.num_files {
                log::warn!("VHFIter has collected too many pages.");
                return Err(Error::ExcessData);
            }

            // If no elements have yet been written, we fill the internal buffer instead.
            if self.num_elements_written.load(Ordering::Acquire) == 0 {
                self.write_data_maybe_buffer(data)
            } else {
                self.write_data_passed_buffer(data)
            }?;

            if self.should_deplete_m_overflow_to_write() {
                self.deplete_from_m_overflow_to_write()?;
            }

            if !data.is_empty() {
                log::trace!(
                    "One round of drain occurred with elements left = {}",
                    data.len()
                )
            }

            // Check if to continue or break loop
            if self.num_elements_written.load(Ordering::Acquire) >= self.num_elements_per_file {
                self.close_file()?;
            }

            if data.is_empty() {
                break 'data_has_element;
            }
            log::trace!("write_data loop continue");
        }

        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        self.close_file()
    }
}

impl<'a> V2BinWriter<'a> {
    /// Creates an object that allows for writing of data processed out of
    /// [crate::runner::process::VHF]. Whilst still working out if [crate::runner::Config] contains
    /// enough information about the runtime, the `main()` function should instead be responsible
    /// for determining the time by which the first data point is being written to file. This means
    /// that data points being dropped in processing should be accounted for.
    pub fn new(config: V2BinArg<'a>, start_time: jiff::Zoned) -> Self {
        debug_assert_ne!(*config.verbosity & 0b100, 0);

        // Constant is currently hard-baked with reference to Archive/20250208, instead of
        // being from config specification.
        let m_overflow_total = if *config.verbosity & 0b1000 != 0 {
            const DEFAULT_RATIO: f64 = 0.00005;
            let m_of_to_d_ratio = config.overflow_to_data_ratio.unwrap_or(DEFAULT_RATIO); // m_overflow to data ratio.
            (*config.num_samples as f64 * m_of_to_d_ratio).round() as usize
        } else {
            0
        };

        Self {
            start_time: Box::new(start_time),
            board_config: Box::new(config.board_config),
            num_files: (*config.num_files).max(1), // Allow for user to not specify
            num_files_so_far: AtomicUsize::new(0),
            time_between_files: Box::new(*config.file_timespan),
            num_elements_per_file: *config.num_samples,
            num_elements_written: AtomicUsize::new(0),
            verbosity: *config.verbosity,
            filename_details: config.filename_details.to_string(),
            elements_to_write: Arc::new(Mutex::new(Vec::with_capacity(
                FILE_LAZY_LEN.min(*config.num_samples),
            ))),
            m_overflow_to_write: Arc::new(Mutex::new(VecDeque::with_capacity(
                FILE_LAZY_LEN.min(*config.num_samples),
            ))),
            file_dir: config.save_dir.to_path_buf(),
            current_file_handle: Arc::new(Mutex::new(None)),
            header_len: None,
            m_offset: 0,
            m_overflow_total,
            m_overflow_written: AtomicUsize::new(0),
        }
    }

    /// Tries to open a file in the specified location with the required name. Fails if file
    /// already exists.
    fn open_file(&mut self) -> Result<BufWriter<File>> {
        if self.current_file_handle.lock().unwrap().is_some() {
            log::error!("A file is being requested to open when it has already been opened.");
            return Err(Error::InternalInconsistency);
        }

        let file_time = self
            .start_time
            .checked_add(
                self.board_config
                    .time_between_vhf_start_and_first_element()?,
            ) // First element written to file
            .map_err(Error::Jiff)?
            .checked_add(
                self.num_files_so_far.load(Ordering::Acquire) as i64 * *self.time_between_files,
            ) // First element of subsequent file
            .map_err(Error::Jiff)?;
        let path = {
            let mut tmp = self.file_dir.clone();
            // Since this function opens the file, we can take this as the offset.
            tmp.push(match self.filename_details.len() {
                0 => file_time.strftime("%FT%T%.f%z").to_string() + ".vhf.bin",
                _ => format!(
                    "{}_{}.vhf.bin",
                    file_time.strftime("%FT%T%.f%z"),
                    self.filename_details
                ),
            });
            tmp
        };

        #[cfg(not(test))]
        {
            let now = Zoned::now();
            if file_time.duration_since(&now) > SignedDuration::new(10, 0)
                || now.duration_since(&file_time) > SignedDuration::new(10, 0)
            {
                log::error!("Created file time differs significantly from current time!");
                return Err(Error::InternalInconsistency);
            }
        }

        if std::fs::exists(path.clone()).map_err(Error::Io)? {
            log::error!("Created file name found to already exist in location, path = {path:?}");
            log::error!(
                "self.start_time = {}, self.time_between_files = {}, num_files_so_far = {}",
                self.start_time,
                self.time_between_files,
                self.num_files_so_far.load(Ordering::Acquire)
            );
            return Err(Error::InternalInconsistency);
        }

        log::info!("Creating file with name {:?}", &path);
        let f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(Error::Io)?;
        self.num_files_so_far.fetch_add(1, Ordering::AcqRel);
        Ok(BufWriter::new(f))
    }

    fn write_header(&mut self) -> Result<()> {
        // Do not write into a file who has already had headers/data written.
        if self.current_file_handle.lock().unwrap().is_none()
            || self.num_elements_written.load(Ordering::Acquire) > 0
        {
            return Err(Error::InternalInconsistency);
        }

        let file_start_time = self
            .start_time
            .checked_add(
                self.board_config
                    .time_between_vhf_start_and_first_element()?,
            ) // First element written to file
            .map_err(Error::Jiff)?
            .checked_add(
                *self.time_between_files
                    // Because the file has already been opened, we have to sub by 1.
                    * self.num_files_so_far
                        .load(Ordering::Acquire)
                        .checked_sub(1)
                        .unwrap() as i64,
            ) // First element of subsequent file
            .map_err(Error::Jiff)?;
        let header_details = {
            let bin_header = V2BinHeader {
                file_start: &file_start_time,
                board_config: &self.board_config,
                m_overflow_total: self.m_overflow_total,
                m_offset: self.m_offset,
            };

            serde_json::to_string(&bin_header)
        }
        .map_err(|_| {
            log::error!("Serialization error!");
            Error::InternalInconsistency
        })?;
        if header_details.len() as u64 > u64::MAX {
            return Err(Error::ExcessData);
        }
        let file_start_unix: jiff::Timestamp = file_start_time.into();

        use byteorder::{NativeEndian, WriteBytesExt};
        use std::io::Write;
        if let Some(buf_file) = self.current_file_handle.lock().unwrap().as_mut() {
            let mut written_so_far = 0;
            buf_file
                .write(V2_MAGIC_HEADER.as_bytes())
                .map_err(Error::Io)?; // Write magic in UTF.
            written_so_far += V2_MAGIC_HEADER.len();
            buf_file
                .write_u16::<NativeEndian>(0xFEFF)
                .map_err(Error::Io)?; // Write BOM.
            written_so_far += 2;
            buf_file
                .write_i64::<NativeEndian>(file_start_unix.as_second())
                .map_err(Error::Io)?; // Write UNIX time stamp.
            written_so_far += 8;
            buf_file
                .write_u64::<NativeEndian>(header_details.len() as u64)
                .map_err(Error::Io)?; // Write header length.
            written_so_far += 8;
            buf_file
                .write_all(header_details.as_bytes())
                .map_err(Error::Io)?; // Write header details.
            written_so_far += header_details.len();
            // Flush to next word.
            let num_zero_bytes = written_so_far.div_ceil(8) * 8 - written_so_far;
            buf_file
                .write_all(&vec![0u8; num_zero_bytes])
                .map_err(Error::Io)?;
            // This is to determine eventually how to offset into the appropriate location within
            // m_idx_overflow block.
            self.header_len = Some(unsafe {
                // SAFETY: Nonzero is guaranteed by V2_MAGIC_HEADER.
                NonZeroUsize::new(written_so_far.div_ceil(8) * 8).unwrap_unchecked()
            });

            // Write m_overflow_idx block.
            if self.verbosity & 0b1000 != 0 {
                buf_file
                    .write_all(&vec![0u8; self.m_overflow_total * 8])
                    .map_err(Error::Io)?;
            } else {
                debug_assert_eq!(self.m_overflow_total, 0);
            }
        }

        Ok(())
    }

    /// This differs from [self]::num_elements_written as this shows the number of elements in
    /// relation to VHFIter start.
    ///
    /// # Assumptions
    /// Assume all files written are exactly num_elements_per_file.
    ///
    /// # Returns
    /// Ok: number of elements with respect to VHFIter start (and processed) written.
    /// Err: usize overflow occurred.
    #[allow(dead_code)]
    fn total_elements_written(&self) -> Result<usize> {
        self.num_files_so_far
            .load(Ordering::Acquire)
            .saturating_sub(1)
            .checked_mul(self.num_elements_per_file)
            .and_then(|s| s.checked_add(self.num_elements_written.load(Ordering::Acquire)))
            .ok_or(Error::ExcessData)
    }

    /// MOverflowRaw encodes the absolute position relative to start of
    /// [super::super::process::VHFIter]. However, we sometimes instead want the absolute position
    /// relative to start of the file. This iterator version avoids a repeated check file start
    /// index relative to VHFIter start.
    #[inline]
    fn align_to_file_start_iter(
        &self,
        m_raws: impl Iterator<Item = MOverflowRaw>,
    ) -> impl Iterator<Item = MOverflowRaw> {
        let neg_offset = self
            .num_files_so_far
            .load(Ordering::Acquire)
            .checked_mul(self.num_elements_per_file)
            .expect("Error trying to get idx of file start relative to VHFIter.");
        m_raws.map(move |m_raw| m_raw.offset_neg(neg_offset))
    }

    /// Takes the absolute index (index relative to VHFIter start) and pushes as is onto
    /// self.m_overflow_to_write.
    // Need to be careful as to when m_idxs crosses file boundaries.
    // Store absolute index on self.m_overflow_to_write, write to file with offset handled.
    fn push_onto_m_overflow_to_write(
        &mut self,
        mut m_idxs: impl Iterator<Item = MOverflowRaw>,
    ) -> Result<()> {
        // Acquire lock only when Iterator has anything
        if let Some(first) = m_idxs.next() {
            if let Ok(mut heap) = self.m_overflow_to_write.lock() {
                heap.push_back(first);
                m_idxs.for_each(|e| heap.push_back(e));
            } else {
                log::warn!("Failed to acquire lock to push onto heap. This should not happen!");
                return Err(Error::InternalInconsistency);
            }
        }

        Ok(())
    }

    /// Tries to read the length of m_overflow_to_write. If the current length is in excess of the
    /// intended capacity X, returns true.
    /// Returns false if lock cannot be acquired.
    fn should_deplete_m_overflow_to_write(&self) -> bool {
        if let Ok(m_idxs) = self.m_overflow_to_write.lock() {
            m_idxs.len() > FILE_LAZY_LEN.min(self.num_elements_per_file) / 2
        } else {
            false
        }
    }

    /// Determines if the final word in file's m_overflow_idx block has been written.
    fn file_m_overflow_has_capacity(&self) -> bool {
        self.m_overflow_written.load(Ordering::Acquire) < self.m_overflow_total
    }

    /// Gets the position of where the next byte in m_overflow_idx should be for writing to.
    /// This is UB when header has not been written.
    fn m_overflow_block_pos(&self) -> u64 {
        let hl: usize = unsafe { self.header_len.unwrap_unchecked() }.into();
        hl as u64 + self.m_overflow_written.load(Ordering::Acquire) as u64 * 8
    }

    /// Call this to remove from the heap and commit to file when either:
    /// (1): There is too many elements on the heap, and should start being written to file;
    /// (2): File is about to be closed.
    /// Drains from self.m_overflow_to_write up to self.num_elements_written.
    fn deplete_from_m_overflow_to_write(&mut self) -> Result<()> {
        // 1. Current at the end of the file.
        // 2. Jump to position within m_idx block to write m_idx with `idx <
        //    self.num_elements_written`, where m_idx is offset to file_start.
        //    a. Do not write past end of m_idx; continue to drain from self.m_overflow_to_write
        //    b. Update self.m_offset.
        // 3. Restore position to end of file.

        // Get elements that have to be drained out of self.m_overflow_to_write
        let mut m_idxs: Vec<_> = {
            if let Ok(mut ms) = self.m_overflow_to_write.lock() {
                let num_elem_written: usize = self.num_elements_written.load(Ordering::Acquire);
                let deque_idx = match ms.binary_search_by(|v| v.0.cmp(&num_elem_written)) {
                    Ok(i) => i.saturating_add(1),
                    Err(i) => i,
                };
                ms.drain(0..deque_idx).collect()
            } else {
                log::error!("Could not obtain m_overflow_to_write.");
                return Err(Error::InternalInconsistency);
            }
        };

        if self.file_m_overflow_has_capacity() {
            // Write only if there is remaining capacity.

            use byteorder::{NativeEndian, WriteBytesExt};
            use std::io::{Seek, SeekFrom, Write};
            if let Some(file) = self.current_file_handle.lock().unwrap().as_mut() {
                file.flush().map_err(Error::Io)?;
                file.seek(SeekFrom::Start(self.m_overflow_block_pos()))
                    .map_err(Error::Io)?;

                // Write + drain from m_idxs
                let mut m_cumulative = 0i64;
                let num_to_drain = m_idxs.len().min(
                    self.m_overflow_total
                        .saturating_sub(self.m_overflow_written.load(Ordering::Acquire)),
                ); // Drain only as many as writeable.
                self.align_to_file_start_iter(m_idxs.drain(0..num_to_drain))
                    .try_for_each(|m_idx| -> vhf_common::Result<()> {
                        m_cumulative += (m_idx.1) as i64;
                        m_idx.try_into().and_then(|m_write: MOverflowWrite| {
                            file.write_i64::<NativeEndian>(*m_write)
                                .map_err(vhf_common::Error::Io)
                        })
                    })?;
                self.m_overflow_written
                    .fetch_add(num_to_drain, Ordering::AcqRel);
                let m_cumulative = m_idxs
                    .into_iter() // Drain the rest
                    .fold(m_cumulative, |acc, m_idx| acc + (m_idx.1) as i64);
                self.m_offset += m_cumulative; // Update m_offset finally

                file.flush().map_err(Error::Io)?; // Seek back
                file.seek(SeekFrom::End(0)).map_err(Error::Io)?;
            } else {
                log::error!("Could not obtain current_file_handle.");
                return Err(Error::InternalInconsistency);
            }
        } else {
            // File has no capacity, we just update self.m_offset.
            self.m_offset += m_idxs
                .into_iter() // Drain the rest
                .fold(0i64, |acc, m_idx| acc + (m_idx.1) as i64);
        }

        Ok(())
    }

    /// In the case where the buffer has not yet been filled.
    fn write_data_maybe_buffer(&mut self, data: &mut Vec<RawVHFWord>) -> Result<()> {
        let buf_len = self.elements_to_write.lock().unwrap().len();
        let words_len = data.len();

        if buf_len + words_len < FILE_LAZY_LEN.min(self.num_elements_per_file) {
            // Fill into temporary buffer;
            self.elements_to_write.lock().unwrap().append(data);
            return Ok(());
        }

        // Temporary buffer [self.elements_to_write] is full.
        // We now open the file and write the header.
        let file_handle = self.open_file()?;
        *self.current_file_handle.lock().unwrap() = Some(file_handle);
        self.write_header()?;

        // Write the data
        use byteorder::{NativeEndian, WriteBytesExt};
        if let Some(file) = self.current_file_handle.lock().unwrap().as_mut() {
            if !self.elements_to_write.lock().unwrap().is_empty() {
                self.elements_to_write
                    .lock()
                    .unwrap()
                    .drain(0..)
                    .try_for_each(|word| file.write_u64::<NativeEndian>(word.into()))
                    .map_err(Error::Io)?;
                self.num_elements_written
                    .fetch_add(buf_len, Ordering::Release);
            };
            // Note! This can be be zero if the internal buffer was just nice the size
            // of the file.
            data.drain(
                ..self
                    .num_elements_per_file
                    .saturating_sub(self.num_elements_written.load(Ordering::Acquire))
                    .min(data.len()),
            )
            .try_for_each(|word| file.write_u64::<NativeEndian>(word.into()))
            .map_err(Error::Io)?;

            self.num_elements_written
                .fetch_add(words_len, Ordering::Release);
        } else {
            // File should have been created.
            log::error!("File should have been created!");
            return Err(Error::InternalInconsistency);
        };
        Ok(())
    }

    /// The amount of written data no longer necessitates writing into the internal buffer.
    fn write_data_passed_buffer(&mut self, data: &mut Vec<RawVHFWord>) -> Result<()> {
        // Elements have been written, we instead just pass straight to the file.
        let words_len = data.len();

        use byteorder::{LittleEndian, WriteBytesExt};
        if let Some(file) = self.current_file_handle.lock().unwrap().as_mut() {
            data.drain(
                0..self
                    .num_elements_per_file
                    .saturating_sub(self.num_elements_written.load(Ordering::Acquire))
                    .min(data.len()),
            )
            .try_for_each(move |word| file.write_u64::<LittleEndian>(word.into()))
            .map_err(Error::Io)?;

            self.num_elements_written
                .fetch_add(words_len, Ordering::Release);
            Ok(())
        } else {
            // File should have been created.
            log::error!("File should have been created!");
            Err(Error::InternalInconsistency)
        }
    }

    fn close_file(&mut self) -> Result<()> {
        self.deplete_from_m_overflow_to_write()?;
        if let Some(file) = self.current_file_handle.lock().unwrap().as_mut() {
            use std::io::Write;
            file.flush().map_err(Error::Io)?;
        }
        self.num_elements_written.fetch_min(0, Ordering::AcqRel);
        self.header_len = None;
        *self.current_file_handle.lock().unwrap() = None;
        Ok(())
    }
}

impl Drop for V2BinWriter<'_> {
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

impl Debug for V2BinWriter<'_> {
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
pub struct V2BinArg<'a> {
    pub board_config: BoardConfig<'a>,
    pub num_samples: &'a usize,
    pub num_files: &'a usize,
    pub verbosity: &'a u8,
    pub overflow_to_data_ratio: &'a Option<f64>,
    pub file_timespan: Box<Span>,
    pub filename_details: String,
    pub save_dir: &'a Path,
}

#[derive(Serialize)]
struct V2BinHeader<'a> {
    /// For the 0th file, this need not be the time the VHF engine starts.
    // It might be easier to just use PyO3 to deserialize this with serde than to use orjson in
    // Python.
    file_start: &'a Zoned,
    #[serde(flatten)]
    board_config: &'a BoardConfig<'a>,
    m_overflow_total: usize,
    m_offset: i64,
}
