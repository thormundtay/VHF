//! WriterBuilder is to help determine at runtime which form of output is to be delegated to.
//! These can be configured through the `.ini` file configuration, or with command line arguments.
//! For more details, please see [crate::runner::Config].

use super::super::{Config, config::typedef::Encode};
use super::VHFWriter;
use super::{V1StdOut, v1_stdout::V1StdOutArg};
use super::{V1Writer, v1_writer::V1Arg};
use super::{V2BinWriter, v2_bin_writer::V2BinArg};
use crate::{Error, Result};
use jiff::Zoned;

/// Please see [WriterBuilder].
#[derive(Debug, Clone)]
pub(in super::super) enum Writers<'a> {
    /// This uses the same structure as V1Writer, but has the complications associated with issue
    /// #23. [Config] should not invoke Stdout writer to the best of its ability.
    V1Stdout(V1StdOutArg<'a>),
    /// V1Output, but to more than 1 file with file-size guarantees.
    V1(V1Arg<'a>),
    V2Bin(V2BinArg<'a>),
}

/// Parameters and validation checking associated with getting the appropriate file writer
/// Note that there is no check that logging is also writing to the same file descriptor, which
/// might be an issue especially in the case of writing to stdout.
pub struct WriterBuilder<'a> {
    /// This is the start time expected by all writers.  
    /// Example: [V1Writer::start_time]
    start_time: Option<Zoned>,
    /// The variant to be determined shall be the responsibility of [Config].
    pub(in super::super) writer_type: Writers<'a>,
}

impl<'a> WriterBuilder<'a> {
    /// Determine (and possibly reject) based on [Config], the relevant file writer to use.
    fn writer_type(conf: &'a Config) -> Result<Writers<'a>> {
        // Prioritize by decreasing verbosity followed by save_to_file.
        match conf {
            Config {
                verbosity: 16.., ..
            } => {
                log::error!("verbosity >= 16 is not supported.");
                Err(Error::User)
            }
            Config {
                num_files: 0..,
                save_to_file: true,
                verbosity: 4..=15,
                ..
            } => {
                unimplemented!("V2Writer not yet implemented.")
            }
            Config {
                num_files: 0..=1,
                save_to_file: false,
                verbosity: 4..=7,
                ..
            } => {
                unimplemented!("V2Stdout not yet implemented.")
            }
            Config {
                num_files: 2..,
                save_to_file: false,
                verbosity: 4..=7,
                ..
            } => {
                log::error!("V2 stdout conflicts with num_files > 1.");
                Err(Error::User)
            }
            Config {
                save_to_file: false,
                verbosity: 8..,
                ..
            } => {
                log::error!(
                    "Verbosity = 8 demands Seekable on file writer, which cannot be done with Stdout. Consider writing to file instead."
                );
                Err(Error::User)
            }
            Config {
                num_files: 0..,
                save_to_file: true,
                verbosity: 0..=3,
                encode: Encode::Binary,
                ..
            } => Ok(Writers::V1(V1Arg {
                num_samples: &conf.num_samples,
                num_files: &conf.num_files,
                encode: &conf.encode,
                verbosity: &conf.verbosity,
                file_timespan: Box::new(conf.file_timespan()),
                header_details: conf.details(),
                filename_details: conf.filename(),
                save_dir: &conf.save_dir,
            })),
            Config {
                num_files: 0..,
                save_to_file: true,
                verbosity: 0..=3,
                encode: Encode::Hexadecimal | Encode::ASCII,
                ..
            } => {
                log::error!("Non-binary encoding not yet implemented.");
                Err(Error::User)
            }
            Config {
                num_files: 0..=1,
                save_to_file: false,
                verbosity: 0..=3,
                encode: Encode::Binary,
                ..
            } => Ok(Writers::V1Stdout(V1StdOutArg {
                encode: &conf.encode,
                verbosity: &conf.verbosity,
                header_details: conf.details(),
            })),
            Config {
                num_files: 0..=1,
                save_to_file: false,
                verbosity: 0..=3,
                encode: Encode::Hexadecimal | Encode::ASCII,
                ..
            } => {
                log::error!("Non-binary encoding not yet implemented.");
                Err(Error::User)
            }
            Config {
                num_files: 2..,
                save_to_file: false,
                verbosity: 0..=3,
                ..
            } => {
                log::error!("V1 stdout conflicts with num_files > 1.");
                Err(Error::User)
            }
        }
    }

    /// This is for producing [Self::build], as `./stream` requires conf to determine the
    /// file_writer type, whilst also having to time_start to already be known.
    ///
    /// This is currently a separate impl to separate out the logic from Config; but the end user
    /// should only just have to pass &conf to [Config]::writer(&self, time_start) -> impl VHFWriter
    pub(crate) fn new(conf: &'a Config) -> Result<Self> {
        let writer_type: Writers = WriterBuilder::writer_type(conf)?;
        Ok(Self {
            start_time: None,
            writer_type,
        })
    }

    pub fn with_start_time(self, start_time: jiff::Zoned) -> Self {
        Self {
            start_time: Some(start_time),
            ..self
        }
    }

    /// Gets a FileWriter.
    /// # Panics
    /// If any required field has not yet been inserted.
    pub fn build(self) -> Box<dyn VHFWriter> {
        match self.writer_type {
            Writers::V2Bin(v2binarg) => todo!(),
            Writers::V1(v1arg) => Box::new(V1Writer::new(v1arg, self.start_time.unwrap().clone())),
            Writers::V1Stdout(v1stdoutarg) => {
                Box::new(V1StdOut::new(v1stdoutarg, self.start_time.unwrap().clone()))
            }
        }
    }
}
