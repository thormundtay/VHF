//! Interface with VHF, with things such as starting the VHF, reading out from it, closing it.

mod consts;
mod pages;

use super::Config;
use crate::{Error, Result};
use mmap_rs::Mmap;
use nix::fcntl;
use std::io::{BufWriter, Write};

/// Everything necessary to ensure the lifetime of pulling memory out from the VHF for its runtime
pub struct VHF {
    configuration: Config,
    pub handle: libc::c_int,
    pub raw_handle: std::fs::File,
    pub readback: Mmap,
}

impl VHF {
    pub fn new(config: Config) -> Result<Self> {
        let handle = Self::open_dev(
            config
                .board
                .clone()
                .into_os_string()
                .to_str()
                .ok_or_else(|| Error::ParseEmpty)?,
        )?; // OsStr -> &str validation should be done by Config
        use std::os::unix::io::FromRawFd;
        let raw_handle = unsafe { std::fs::File::from_raw_fd(handle) };
        let readback = Self::readback_buffer(&raw_handle)?;
        log::debug!("Readback buffer created");

        Ok(Self {
            configuration: config,
            handle,
            raw_handle,
            readback,
        })
    }

    /// For a path representing a device `dev`, such as `/dev/usbhybrid0`, returning the file
    /// handle.
    fn open_dev(dev: &str) -> Result<libc::c_int> {
        Ok(fcntl::open(
            dev,
            fcntl::OFlag::O_RDWR,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .map_err(|e| Error::CIo(e))?)
    }

    fn readback_buffer(raw_handle: &std::fs::File) -> Result<Mmap> {
        let mmap_options = unsafe {
            mmap_rs::MmapOptions::new(1 << 22)
                .map_err(Error::MMap)?
                .with_file(raw_handle, 0)
                .with_flags(mmap_rs::MmapFlags::SHARED)
        };
        let mut result = mmap_options.map().map_err(Error::MMap)?;
        result.lock().map_err(Error::MMap)?; // Make RAM only

        Ok(result)
    }

    /// Start USB Machine, with all the specified configuration
    pub fn start(&self) -> Result<()> {
        consts::ioctl_start(self.handle).map(|_| ())?;
        {
            let mut buf_write = BufWriter::new(self.raw_handle.try_clone().map_err(Error::Io)?);
            buf_write.write(b"clockinit; adcinit;").map_err(Error::Io)?;
            buf_write.flush().map_err(Error::Io)?;
            std::thread::sleep(std::time::Duration::from_nanos(2000)); // 1000 might be sufficient

            buf_write.write(b"config 16; param 1;").map_err(Error::Io)?;
            buf_write.write(b"config 1; param 9;").map_err(Error::Io)?; // skips 9 samples
            buf_write.write(b"config 2; param 0;").map_err(Error::Io)?; // Gain parameter = 0
            buf_write.write(b"config 3; param 0;").map_err(Error::Io)?; // debug param = 0
            buf_write.write(b"skip; skip;").unwrap();
            buf_write.flush().map_err(Error::Io)?;

            buf_write
                .write(format!("cstream {};", 0x120).as_bytes())
                .map_err(Error::Io)?;
            buf_write.flush().map_err(Error::Io)?;
        }

        Ok(())
    }

    /// Stops USB Machine and close FDs.
    pub fn stop(&self) -> Result<()> {
        self.raw_handle
            .try_clone()
            .map_err(Error::Io)?
            .write(b"stop; config 0;")
            .map_err(Error::Io)?;
        consts::ioctl_end(self.handle).map(|_| ())
    }

    /// Assumes the USB Machine has started.
    /// Gets the next index to read up to as given by ioctl
    #[inline(always)]
    pub fn ioctl_next(&self) -> Result<libc::c_int> {
        consts::ioctl_read(self.handle)
    }
}
