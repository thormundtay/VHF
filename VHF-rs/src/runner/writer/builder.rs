//! WriterBuilder is to help determine at runtime which form of output is to be delegated to.
//! These can be configured through the `.ini` file configuration, or with command line arguments.
//! For more details, please see [crate::runner::Configs].

use super::super::{Config, config::typedef::Encode};
use super::{V1StdOut, V1Writer, VHFWriter};
use crate::{Error, Result};
use jiff::Zoned;

#[derive(Debug, Clone)]
pub(super) struct V1StdOutArg<'a> {
    pub encode: &'a Encode,
    pub verbosity: &'a u8,
}

#[derive(Debug, Clone)]
pub(super) struct V1Arg<'a> {
    pub num_files: &'a usize,
    pub encode: &'a Encode,
    pub verbosity: &'a u8,
}

/// Please see [WriterBuilder].
#[derive(Debug, Clone)]
pub(super) enum Writers<'a> {
    /// This uses the same structure as V1Writer, but has the complications associated with issue
    /// #23. [Config] should not invoke Stdout writer to the best of its ability.
    V1Stdout(V1StdOutArg<'a>),
    /// V1Output, but to more than 1 file with file-size guarantees.
    V1(V1Arg<'a>),
}

/// Parameters and validation checking associated with getting the appropriate file writer
/// Note that there is no check that logging is also writing to the same file descriptor, which
/// might be an issue especially in the case of writing to stdout.
pub struct WriterBuilder<'a> {
    /// This is the start time expected by all writers.  
    /// Example: [v1_writer::V1Writer::start_time]
    start_time: Option<Zoned>,
    /// The variant to be determined shall be the responsibility of [Config].
    writer_type: Writers<'a>,
}

impl<'a> WriterBuilder<'a> {
    /// This is for producing [Self::build], as `./stream` requires conf to determine the
    /// file_writer type, whilst also having to time_start to already be known.
    ///
    /// This is currently a separate impl to separate out the logic from Config; but the end user
    /// should only just have to pass &conf to [Config]::writer(&self, time_start) -> impl VHFWriter
    pub(super) fn new(conf: &'a Config) -> Result<Self> {
        todo!()
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
        todo!()
    }
}
