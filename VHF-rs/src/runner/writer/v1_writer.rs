//! Writer method meant to be as identical as possible to the original C file writer.

use super::{VHFWriter, FILE_LAZY_LEN};
use crate::{runner::Config, types::RawVHFWord, Error, Result};
use jiff::{Span, Zoned};
use std::{
    fs::File,
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
        todo!()
    }
}
