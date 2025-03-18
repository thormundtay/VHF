//! Interface with VHF, with things such as starting the VHF, reading out from it, closing it.

mod consts;
mod mmap_reader;
mod pages;

use self::mmap_reader::mmap_thread;
use self::pages::MmapPage;
use super::Config;
use crate::{Error, Result};
use mmap_rs::Mmap;
use nix::fcntl;
use std::collections::VecDeque;
use std::io::{BufWriter, Write};
use std::num::NonZeroU64;
use std::sync::{
    atomic::{self, AtomicBool},
    Arc, Condvar, Mutex,
};
use std::thread::{self, JoinHandle};

/// This is the size in bytes of the Mmap that is backed by the VHF device.
const MMAP_BYTES_LEN: usize = 1 << 22;

/// Everything necessary to ensure the lifetime of pulling memory out from the VHF for its runtime
pub struct VHF {
    configuration: Config,
    handle: libc::c_int,
    // It might be possible that File as created by Handle in mmap_thread might lead to a double
    // close. (remove comment after test. remove pub after test.)
    raw_handle: std::fs::File,
    /// map_reader contains the thread that is responsible for pulling elements out of the MMap
    /// into a [buffer].
    /// More details is as given in [self::mmap_reader].
    map_reader: JoinHandle<Result<()>>,
    /// Used to signal to [self::mmap_reader::MMapReader] has started, and to determine that child has stopped.
    engine_running: Arc<AtomicBool>,
    /// Used to receive signal from [self::mmap_reader::MMapReader] that new pages have been placed into
    /// [self.buffer].
    buffer_signal: Arc<Condvar>,
    /// buffer is a local mirror of Mmap that is intended for the likes of SlidingWindow
    /// [itertools::tuple_windows] and par_map, which has more Rust Semantics than reading straight
    /// out of a Mmap.
    buffer: Arc<Mutex<VecDeque<MmapPage>>>,
    /// This is the total number of pages to be read by [self::MMapReader].
    total_to_read: NonZeroU64,
}

impl VHF {
    /// Create a new instance of VHF control. The goal of [VHF] is to create all necessary control
    /// flow to read out of Mmap and to stream in the `impl Iterator for VHF` trait.
    pub fn new(config: Config) -> Result<Self> {
        let handle = Self::open_dev(
            config
                .board
                .clone()
                .into_os_string()
                .to_str()
                .ok_or_else(|| Error::ParseEmpty)?,
        )?; // TODO: OsStr -> &str validation should be done by Config

        // WARN: Hardcoded for now.
        // This is the number of heap-allocated pages being emitted from the [self::MMapReader].
        let total_to_read = unsafe { NonZeroU64::new(1 << 23).unwrap_unchecked() };

        let engine_running = Arc::new(AtomicBool::new(false));
        let raw_handle = {
            use std::os::unix::io::FromRawFd;
            unsafe { std::fs::File::from_raw_fd(handle) }
        };
        let buffer = {
            let mut buffer = VecDeque::with_capacity(256);
            // WARN: Number of Empty pages should be given by the transform. Currently hardcoded.
            buffer.push_back(MmapPage::Empty);
            Arc::new(Mutex::new(buffer))
        };
        let buffer_signal = Arc::new(Condvar::new()); // merge into buffer?

        let map_reader: JoinHandle<Result<()>> = {
            let readback = Self::readback_buffer(&raw_handle)?;
            let buffer_signal = buffer_signal.clone();
            let buffer = buffer.clone();
            let engine_running = engine_running.clone();
            thread::Builder::new()
                .name("mmap_reader".to_string())
                .spawn(move || {
                    mmap_thread(
                        readback,
                        engine_running,
                        buffer_signal,
                        buffer,
                        total_to_read,
                        handle,
                    )
                })
                .map_err(Error::Io)
        }?;

        log::debug!("Readback buffer created");

        Ok(Self {
            configuration: config,
            handle,
            raw_handle,
            map_reader,
            engine_running,
            buffer_signal,
            buffer,
            total_to_read,
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

    // We do not want both the parent(main) thread and child thread to have to hold ownership of
    // the MmapMut, which would mean that this struct would have to consistently reach into an
    // Arc<Mutex<_>> just to read out of the MMapMut.
    fn readback_buffer(raw_handle: &std::fs::File) -> Result<Mmap> {
        let mmap_options = unsafe {
            mmap_rs::MmapOptions::new(MMAP_BYTES_LEN)
                .map_err(Error::MMap)?
                .with_file(raw_handle, 0)
                .with_flags(mmap_rs::MmapFlags::SHARED)
        };
        let mut result = mmap_options.map().map_err(Error::MMap)?;
        result.lock().map_err(Error::MMap)?; // Make RAM only

        Ok(result)
    }

    /// Start USB Machine, with all the specified configuration.
    // We spawn a thread here that reads off from the MmapMut into our own "buffer", which gets
    // sliding window overed before being passed to a transformer (in either a map or par_map).
    pub fn start(&self) -> Result<()> {
        if self.engine_running.load(atomic::Ordering::Acquire) {
            return Err(Error::EngineRunning);
        };

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
        self.engine_running.store(true, atomic::Ordering::Release);
        self.map_reader.thread().unpark();

        Ok(())
    }

    /// Stops USB Machine and close FDs.
    // Might want to consider moving this routine as to being called from the MmapMut reader thread
    // instead of being called from the main() function.
    pub fn stop(&self) -> Result<()> {
        if !self.engine_running.load(atomic::Ordering::Acquire) {
            return Err(Error::EngineStopped);
        };

        // Stop USB device.
        self.raw_handle
            .try_clone()
            .map_err(Error::Io)?
            .write(b"stop; config 0;")
            .map_err(Error::Io)?;
        // Stop hostside USB device.
        let result = consts::ioctl_end(self.handle).map(|_| ());

        self.engine_running.store(false, atomic::Ordering::Relaxed);

        log::info!("VHF stopped!");

        result
    }

    /// Assumes the USB Machine has started.
    /// Gets the next index to read up to as given by ioctl
    #[inline(always)]
    pub fn ioctl_next(&self) -> Result<libc::c_int> {
        consts::ioctl_read(self.handle)
    }
}
