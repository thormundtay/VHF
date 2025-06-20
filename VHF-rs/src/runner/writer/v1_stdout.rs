//! Writer method meant to be as identical as possible to the original C file writer, but designed
//! specifically for stdout writing.

use super::super::config::typedef::Encode;
use super::{V1_MAGIC_HEADER, VHFWriter};
use crate::{Error, Result};
use jiff::Zoned;
use std::{
    io::{BufWriter, Stdout},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct V1StdOut {
    /// Timestamp of the first file's first datapoint.
    start_time: Zoned,
    verbosity: u8,
    header_details: String,
    #[allow(dead_code)]
    encode: Encode,
    stdout: BufWriter<Stdout>,
    has_written: AtomicBool,
}

impl VHFWriter for V1StdOut {
    /// # Silent errors
    /// Does not respect if encode is not Binary.
    fn new(config: &crate::runner::Config, start_time: jiff::Zoned) -> Self {
        Self {
            start_time,
            verbosity: config.verbosity,
            header_details: config.details(),
            encode: config.encode,
            stdout: BufWriter::new(std::io::stdout()),
            has_written: AtomicBool::new(false),
        }
    }

    fn close(&mut self) -> Result<()> {
        use std::io::Write;
        self.stdout.flush().map_err(Error::Io)?;
        Ok(())
    }

    fn write_data(&mut self, words: super::WriteBlock) -> Result<()> {
        if !self.has_written.load(Ordering::Acquire) {
            self.write_header()?;
        }

        use byteorder::{LittleEndian, WriteBytesExt};
        words
            .data
            .into_iter()
            .try_for_each(move |word| self.stdout.write_u64::<LittleEndian>(word))
            .map_err(Error::Io)
    }
}

impl V1StdOut {
    fn write_header(&mut self) -> Result<()> {
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
                self.start_time.strftime("%FT%T%z").to_string(),
                "\n".to_string(),
            ]);
        }
        // + 1 to include the magic mask length
        let header_len = header.len().div_ceil(8) + 1;
        if header_len > 0xffff {
            log::error!("Too much data written into header!");
            return Err(Error::ExcessData);
        };

        use byteorder::{LittleEndian, WriteBytesExt};
        use std::io::Write;
        let header_first = V1_MAGIC_HEADER | (header_len as u64);
        self.stdout
            .write_u64::<LittleEndian>(header_first)
            .map_err(Error::Io)?;
        self.stdout
            .write_all(header.as_bytes())
            .map_err(Error::Io)?;
        let pad = header.len().div_ceil(8) * 8 - header.len();
        self.stdout.write(&vec![0u8; pad]).map_err(Error::Io)?;

        self.has_written.store(true, Ordering::Release);

        Ok(())
    }
}
