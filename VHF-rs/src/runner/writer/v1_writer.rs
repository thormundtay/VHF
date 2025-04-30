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
        let data: &mut Vec<_> = &mut words.data;
        'data_has_element: loop {
            // If no elements have yet been written, we fill the internal buffer instead.
            if self.num_elements_written == 0 {
                if self.num_files_so_far > self.num_files {
                    return Err(Error::ExcessData);
                }

                let buf_len = self.elements_to_write.len();
                let words_len = data.len();
                if buf_len + words_len < FILE_LAZY_LEN {
                    // Fill into temporary buffer;
                    self.elements_to_write.extend(data.drain(..));
                    return Ok(());
                } else {
                    // Buffer is full.
                    let file_handle = self.open_file()?;
                    self.current_file_handle = Some(file_handle);
                    self.write_header()?;

                    // If buffer is full, we now open the file and write the header.
                    use byteorder::{LittleEndian, WriteBytesExt};
                    if let Some(file) = self.current_file_handle.as_mut() {
                        if self.elements_to_write.len() > 0 {
                            self.elements_to_write
                                .drain(0..)
                                .try_for_each(|word| file.write_u64::<LittleEndian>(word))
                                .map_err(Error::Io)?;
                            self.num_elements_written += buf_len;
                        };
                        // Note! This can be be zero if the internal buffer was just nice the size
                        // of the file.
                        data.drain(
                            ..self
                                .num_elements_per_file
                                .saturating_sub(self.num_elements_written)
                                .min(data.len()),
                        )
                        .try_for_each(|word| file.write_u64::<LittleEndian>(word))
                        .map_err(Error::Io)?;

                        self.num_elements_written += words_len;
                    } else {
                        // File should have been created.
                        return Err(Error::InternalInconsistency);
                    };
                }
            } else {
                // Elements have been written, we instead just pass straight to the file.
                let words_len = data.len();

                use byteorder::{LittleEndian, WriteBytesExt};
                if let Some(file) = self.current_file_handle.as_mut() {
                    data.drain(
                        0..self
                            .num_elements_per_file
                            .saturating_sub(self.num_elements_written)
                            .min(data.len()),
                    )
                    .try_for_each(move |word| file.write_u64::<LittleEndian>(word))
                    .map_err(Error::Io)?;

                    self.num_elements_written += words_len;
                } else {
                    // File should have been created.
                    return Err(Error::InternalInconsistency);
                };
            };
            log::trace!(
                "One round of drain occurred. Number of elements left = {}",
                data.len()
            );

            // Check if to continue or break loop
            if self.num_elements_written >= self.num_elements_per_file {
                self.num_elements_written = 0;
                self.close_file()?
            }

            if data.len() == 0 {
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
