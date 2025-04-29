//! Writer method meant to be as identical as possible to the original C file writer.

use super::{VHFWriter, FILE_LAZY_LEN};
use crate::{runner::Config, types::RawVHFWord, Error, Result};
use jiff::{Span, Zoned};
use std::{
    fs::{File, OpenOptions},
    io::BufWriter,
    path::PathBuf,
};

const V1_MAGIC_HEADER: u64 = 0x123456ABCDEF0000;

pub struct V1Writer {
    /// Timestamp of the first file's first datapoint.
    start_time: Zoned,
    /// Total number of files expected.
    num_files: usize,
    /// Number of files that have been opened so far.
    num_files_so_far: usize,
    time_between_files: Span,
    /// This is the number of [crate::runner::Config::num_samples] to be eventually be written to file.
    num_elements_per_file: usize,
    num_elements_written: usize,
    verbosity: u8,
    header_details: String,
    filename_details: String,
    /// Avoid writing to a file until we exceed some amount.
    elements_to_write: Vec<RawVHFWord>,
    file_dir: PathBuf,
    current_file_handle: Option<BufWriter<File>>,
}

impl VHFWriter for V1Writer {
    fn new(config: &Config, start_time: jiff::Zoned) -> Self {
        Self {
            start_time,
            num_files: config.num_files,
            num_files_so_far: 0,
            time_between_files: config.file_timespan(),
            num_elements_per_file: config.num_samples,
            num_elements_written: 0,
            verbosity: config.verbosity,
            header_details: config.details(),
            filename_details: config.filename(),
            elements_to_write: Vec::with_capacity(FILE_LAZY_LEN),
            file_dir: config.save_dir.clone(),
            current_file_handle: None,
        }
    }

    /// Push [super::WriteBlock] onto file.
    fn write_data(&mut self, mut words: super::WriteBlock) -> Result<()> {
        todo!()
    }

    fn close(&mut self) -> Result<()> {
        self.close_file()
    }
}

impl V1Writer {
    /// Tries to open a file in the specified location with the required name. Fails if file
    /// already exists.
    fn open_file(&mut self) -> Result<BufWriter<File>> {
        if self.current_file_handle.is_some() {
            log::error!("A file is being requested to open when it has already been opened.");
            return Err(Error::InternalInconsistency);
        }

        let path = {
            let mut tmp = self.file_dir.clone();
            // Since this function opens the file, we can take this as the offset.
            let time = self
                .start_time
                .checked_add(self.num_files_so_far as i64 * self.time_between_files)
                .map_err(Error::Jiff)?;
            if self.filename_details.len() > 0 {
                tmp.push(format!(
                    "{}_{}.bin",
                    time.strftime("%FT%T%z"),
                    self.filename_details
                ))
            } else {
                tmp.push(time.strftime("%FT%T%z").to_string() + ".bin")
            };
            tmp
        };

        if std::fs::File::open(path.clone()).is_ok() {
            log::error!("Created file name found to already exist in location.");
            return Err(Error::InternalInconsistency);
        }

        log::info!("Creating file with name {:?}", &path);
        let f = OpenOptions::new()
            .create(true)
            .write(true)
            .open(path)
            .map_err(Error::Io)?;
        self.num_files_so_far += 1;
        Ok(BufWriter::new(f))
    }

    /// Writes the file headers. Call this only before the first amount of data is being written.
    fn write_header(&mut self) -> Result<()> {
        // Do not write into a file who has already had headers/data written.
        if self.current_file_handle.is_none() || self.num_elements_written > 0 {
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
                            * self.num_files_so_far.checked_sub(1).unwrap() as i64,
                    )
                    .map_err(Error::Jiff)?
                    .strftime("%FT%T%z")
                    .to_string(),
                "\n".to_string(),
            ]);
        }
        let header_len = header.len().div_ceil(8);
        if header_len > 0xffff {
            log::error!("Too much data written into header!");
            return Err(Error::ExcessData);
        };

        if let Some(buf_file) = self.current_file_handle.as_mut() {
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

    fn close_file(&mut self) -> Result<()> {
        if let Some(file) = self.current_file_handle.as_mut() {
            use std::io::Write;
            file.flush().map_err(Error::Io)?;
        }
        self.current_file_handle = None;
        Ok(())
    }
}
