//! Writer method meant to be as identical as possible to the original C file writer.

use super::{FILE_LAZY_LEN, VHFWriter};
use crate::{Error, Result};
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
use vhf_common::config_types::Encode;
use vhf_common::data_types::RawVHFWord;

pub(super) const V1_MAGIC_HEADER: u64 = 0x123456ABCDEF0000;

pub struct V1Writer {
    /// Timestamp of the first file's first datapoint.
    start_time: Zoned,
    /// Total number of files expected.
    num_files: usize,
    /// Number of files that have been opened so far.
    num_files_so_far: AtomicUsize,
    time_between_files: Span,
    /// This is the number of [crate::runner::Config::num_samples] to be eventually be written to file.
    num_elements_per_file: usize,
    /// This is the number of elements written to the current file.
    num_elements_written: AtomicUsize,
    verbosity: u8,
    header_details: String,
    filename_details: String,
    #[allow(dead_code)]
    encode: Encode,
    /// Avoid writing to a file until we exceed some amount.
    elements_to_write: Arc<Mutex<Vec<RawVHFWord>>>,
    file_dir: PathBuf,
    current_file_handle: Arc<Mutex<Option<BufWriter<File>>>>,
}

impl VHFWriter for V1Writer {
    /// Push [super::WriteBlock] onto file.
    fn write_data(&mut self, mut words: super::WriteBlock) -> Result<()> {
        let data: &mut Vec<_> = &mut words.data;
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

#[derive(Debug, Clone)]
pub struct V1Arg<'a> {
    pub num_samples: &'a usize,
    pub num_files: &'a usize,
    pub encode: &'a Encode,
    pub verbosity: &'a u8,
    pub file_timespan: Box<Span>,
    pub header_details: String,
    pub filename_details: String,
    pub save_dir: &'a Path,
}

impl V1Writer {
    /// Creates an object that allows for writing of data processed out of
    /// [crate::runner::process::VHF]. Whilst still working out if [crate::runner::Config] contains
    /// enough information about the runtime, the `main()` function should instead be responsible
    /// for determining the time by which the first data point is being written to file. This means
    /// that data points being dropped in processing should be accounted for.
    ///
    /// # Silent errors
    /// Does not respect if encode is not Binary.
    pub fn new(config: V1Arg, start_time: jiff::Zoned) -> Self {
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
            encode: *config.encode,
            elements_to_write: Arc::new(Mutex::new(Vec::with_capacity(
                FILE_LAZY_LEN.min(*config.num_samples),
            ))),
            file_dir: config.save_dir.to_path_buf(),
            current_file_handle: Arc::new(Mutex::new(None)),
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
                self.num_files_so_far.load(Ordering::Acquire) as i64 * self.time_between_files,
            )
            .map_err(Error::Jiff)?;
        let path = {
            let mut tmp = self.file_dir.clone();
            // Since this function opens the file, we can take this as the offset.
            tmp.push(match self.filename_details.len() {
                0 => file_time.strftime("%FT%T%.f%z").to_string() + ".bin",
                _ => format!(
                    "{}_{}.bin",
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
        if std::fs::exists(path.clone()).unwrap() {
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

    /// Writes the file headers. Call this only before the first amount of data is being written.
    fn write_header(&mut self) -> Result<()> {
        // Do not write into a file who has already had headers/data written.
        if self.current_file_handle.lock().unwrap().is_none()
            || self.num_elements_written.load(Ordering::Acquire) > 0
        {
            return Err(Error::InternalInconsistency);
        }

        // Early exit
        if self.verbosity == 0 {
            return Ok(());
        }

        let mut header = String::new();
        if self.verbosity & 1 != 0 {
            // Command Line
            header.extend(["# command line: ", self.header_details.as_str(), "\n"]);
        }

        if self.verbosity & 2 != 0 {
            // Time start
            header.extend([
                "# recording start: ".to_string(),
                self.start_time
                    .checked_add(
                        self.time_between_files
                            // Because the file has already been opened, we have to sub by 1.
                            * self.num_files_so_far.load(Ordering::Acquire).checked_sub(1).unwrap() as i64,
                    )
                    .map_err(Error::Jiff)?
                    .strftime("%FT%T%.f%z")
                    .to_string(),
                "\n".to_string(),
            ]);
        }
        // + 1 to include the magic mask length
        let header_len = header.len().div_ceil(8) + 1;
        if header_len > 0xffff {
            log::error!("Too much data written into header!");
            return Err(Error::ExcessData);
        };

        if let Some(buf_file) = self.current_file_handle.lock().unwrap().as_mut() {
            use byteorder::{LittleEndian, WriteBytesExt};
            use std::io::Write;
            let header_first = V1_MAGIC_HEADER | (header_len as u64);
            buf_file
                .write_u64::<LittleEndian>(header_first)
                .map_err(Error::Io)?;
            buf_file.write(header.as_bytes()).map_err(Error::Io)?;
            // Zero pad
            let pad = header.len().div_ceil(8) * 8 - header.len();
            buf_file.write(&vec![0u8; pad]).map_err(Error::Io)?;
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
        use byteorder::{LittleEndian, WriteBytesExt};
        if let Some(file) = self.current_file_handle.lock().unwrap().as_mut() {
            if !self.elements_to_write.lock().unwrap().is_empty() {
                self.elements_to_write
                    .lock()
                    .unwrap()
                    .drain(0..)
                    .try_for_each(|word| file.write_u64::<LittleEndian>(word.into()))
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
            .try_for_each(|word| file.write_u64::<LittleEndian>(word.into()))
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
        if let Some(file) = self.current_file_handle.lock().unwrap().as_mut() {
            use std::io::Write;
            file.flush().map_err(Error::Io)?;
        }
        self.num_elements_written.fetch_min(0, Ordering::AcqRel);
        *self.current_file_handle.lock().unwrap() = None;
        Ok(())
    }
}

impl Drop for V1Writer {
    fn drop(&mut self) {
        if thread::panicking() {
            log::warn!("V1Writer in panic. self = {self:?}");
        }

        let tmp = self.close_file();
        if tmp.is_err() {
            log::warn!("Failed to close file during drop!");
        }
    }
}

impl Debug for V1Writer {
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
            // Don't really care for the internal headers etc as they are const
            .finish()
    }
}
