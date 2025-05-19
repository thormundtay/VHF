//! Interface with VHF, with things such as starting the VHF, reading out from it, closing it.

mod board_ioctl_consts;
pub(super) mod consts;
mod mmap_reader;
pub(super) mod pages;

use super::fold::StreamFold;
use super::Config;
use crate::{Error, Result};
use consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN};
use heapless::Deque;
use itertools::Itertools;
use jiff::{Span, Zoned};
use mmap_reader::mmap_thread;
use mmap_rs::Mmap;
use nix::fcntl;
use pages::MmapPage;
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::{
    atomic::{self, AtomicBool},
    Arc, Condvar, Mutex, RwLock,
};
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// This is the size in bytes of the Mmap that is backed by the VHF device.
const MMAP_BYTES_LEN: usize = 1 << 22;
/// The size of the only continuous ring buffer that is [heapless::Deque].
const DEQUE_CAP: usize = 256;

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
    map_reader: Rc<JoinHandle<Result<()>>>,
    /// Used to signal to [self::mmap_reader::MMapReader] has started, and to determine that child has stopped.
    engine_running: Arc<AtomicBool>,
    /// Stopped invoked
    vhf_stop: bool,
    /// Used to receive signal from [self::mmap_reader::MMapReader] that new pages have been placed into
    /// [self.buffer].
    buffer_signal: Arc<Condvar>,
    /// buffer is a local mirror of Mmap that is intended for the likes of SlidingWindow
    /// [itertools::tuple_windows] and par_map, which has more Rust Semantics than reading straight
    /// out of a Mmap.
    buffer: Arc<Mutex<Deque<MmapPage, DEQUE_CAP>>>,
    /// This is the amount of time between any two pages. Used for determining other timings.
    time_between_pages: Span,
    /// Expected time when to next wake up mmap_reader thread.
    wake_mmap: Arc<RwLock<Instant>>,
    /// This is the total number of pages to be read by [self::MMapReader].
    total_pages_to_read: NonZeroUsize,
}

impl VHF {
    /// Create a new instance of VHF control. The goal of [VHF] is to create all necessary control
    /// flow to read out of Mmap and to stream in the `impl Iterator for VHF` trait.
    /// # Arguments
    /// - config: [Config]
    ///   Configuration for running VHF.
    /// - params: [StreamFold]
    ///   This is to ensure that the same parameters are being used by the driving body and [VHF].
    pub fn new(config: &Config, params: &StreamFold) -> Result<Self> {
        let handle = Self::open_dev(
            config
                .board
                .clone()
                .into_os_string()
                .to_str()
                .ok_or(Error::ParseEmpty)?,
        )?; // TODO: OsStr -> &str validation should be done by Config

        assert_eq!(config.stream_fold_parameters(), params);

        let time_between_pages: Span = pages::time_between_pages_in_ns(&config.speed)?;
        log::debug!("Time between pages = {}", time_between_pages);

        let wake_mmap = Arc::new(RwLock::new(Instant::now()));

        // This will have to be changed as filtering etc means data points are not being passed to
        // file writer.
        let total_elements_to_read =
            unsafe { NonZeroUsize::new(config.total_elements_to_read()).unwrap_unchecked() };
        let total_pages_to_read: NonZeroUsize = unsafe {
            NonZeroUsize::new(usize::from(total_elements_to_read).div_ceil(MMAP_PAGE_LEN))
                .unwrap_unchecked()
        };

        let engine_running = Arc::new(AtomicBool::new(false));
        let raw_handle = {
            use std::os::unix::io::FromRawFd;
            unsafe { std::fs::File::from_raw_fd(handle) }
        };
        let buffer = {
            let mut buffer = Deque::new();
            // Left padding is initialisation, and is thus handled in the parent.
            // Right-padding is termination, and therefore has to be handled by the child thread.
            (0..config.stream_fold.pad())
                .try_for_each(|_| buffer.push_back(MmapPage::Empty))
                .expect("Failed to push_back onto buffer.");
            Arc::new(Mutex::new(buffer))
        };
        let buffer_signal = Arc::new(Condvar::new()); // merge into buffer?

        let map_reader: JoinHandle<Result<()>> = {
            let readback = Self::readback_buffer(&raw_handle)?;
            let engine_running = engine_running.clone();
            let buffer_signal = buffer_signal.clone();
            let buffer = buffer.clone();
            let number_of_pages_between_stream_resume = (VHF_MMAP_WINDOW_LEN as i64 - 4) * 3;
            let time_between_stream_resume = (number_of_pages_between_stream_resume
                * time_between_pages)
                .try_into()
                .map_err(Error::Jiff)?;
            debug_assert!(number_of_pages_between_stream_resume < DEQUE_CAP as i64 / 2);
            log::debug!(
                "MMapReader time_between_stream_resume = {:?}",
                time_between_stream_resume
            );
            let next_collect_time = wake_mmap.clone();
            let streamfold = params.clone();
            thread::Builder::new()
                .name("mmap_reader".to_string())
                .spawn(move || {
                    mmap_thread(
                        readback,
                        engine_running,
                        buffer_signal,
                        buffer,
                        &time_between_pages,
                        time_between_stream_resume,
                        next_collect_time,
                        total_pages_to_read,
                        handle,
                        &streamfold,
                    )
                })
                .map_err(Error::Io)
        }?;

        log::debug!("Readback buffer thread created");

        Ok(Self {
            configuration: config.clone(),
            handle,
            raw_handle,
            map_reader: Rc::new(map_reader),
            engine_running,
            vhf_stop: false,
            buffer_signal,
            buffer,
            time_between_pages,
            wake_mmap,
            total_pages_to_read,
        })
    }

    /// For a path representing a device `dev`, such as `/dev/usbhybrid0`, returning the file
    /// handle.
    fn open_dev(dev: &str) -> Result<libc::c_int> {
        fcntl::open(
            dev,
            fcntl::OFlag::O_RDWR,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .map_err(Error::CIo)
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

        {
            // Busy work to warm the MMap
            let tmp = result
                .iter()
                .take((1 << 22) / 4)
                .step_by(1024)
                .fold(0u64, |mut acc, &x| {
                    acc += x as u64;
                    acc
                });
            log::debug!("Prepopulating mmap summed to: {}", tmp);
        }

        Ok(result)
    }

    /// Start USB Machine, with all the specified configuration.
    /// Returns the time the VHF board has been signalled to start.
    // We spawn a thread here that reads off from the MmapMut into our own "buffer", which gets
    // sliding window overed before being passed to a transformer (in either a map or par_map).
    pub fn start(&self) -> Result<Zoned> {
        if self.engine_running.load(atomic::Ordering::Acquire) {
            return Err(Error::EngineRunning);
        };

        board_ioctl_consts::ioctl_start(self.handle).map(|_| ())?;
        let config = &self.configuration;
        let time_start: Zoned = {
            let mut buf_write = BufWriter::new(self.raw_handle.try_clone().map_err(Error::Io)?);
            buf_write.write(b"clockinit; adcinit;").map_err(Error::Io)?;
            buf_write.flush().map_err(Error::Io)?;
            std::thread::sleep(std::time::Duration::from_nanos(2000)); // 1000 might be sufficient

            buf_write
                .write(format!("config 16; param {};", config.filter_const.unwrap_or(0)).as_bytes())
                .map_err(Error::Io)?; // filter_const
            buf_write
                .write(format!("config 1; param {};", config.skip_num).as_bytes())
                .map_err(Error::Io)?; // skips samples
            buf_write
                .write(format!("config 2; param {};", config.gain.unwrap_or(0)).as_bytes())
                .map_err(Error::Io)?; // Gain parameter
            buf_write
                .write(format!("config 3; param {};", config.gain.unwrap_or(0)).as_bytes())
                .map_err(Error::Io)?; // Gain parameter
            buf_write.write(b"config 3; param 0;").map_err(Error::Io)?; // debug param = 0
            buf_write.write(b"skip; skip;").unwrap();
            buf_write.flush().map_err(Error::Io)?;

            buf_write
                .write(format!("cstream {};", 0x120).as_bytes())
                .map_err(Error::Io)?;
            buf_write.flush().map_err(Error::Io)?;
            Zoned::now()
        };
        self.engine_running.store(true, atomic::Ordering::Release);
        self.unpark_child();

        Ok(time_start)
    }

    /// Closes FDs.
    // Might want to consider moving this routine as to being called from the MmapMut reader thread
    // instead of being called from the main() function.
    pub fn stop(&mut self) -> Result<()> {
        log::info!("VHF stop has been invoked.");
        if self.vhf_stop {
            log::warn!("Parent thread found engine to have already been stopped.");
            return Err(Error::EngineStopped);
        }
        self.vhf_stop = true;

        if self.engine_running.load(atomic::Ordering::Acquire) {
            log::warn!("Parent thread tried to close when engine has not shut down.");
            self.vhf_stop = false;
            return Err(Error::EngineRunning);
        };

        // Stop USB device.
        self.raw_handle
            .try_clone()
            .map_err(Error::Io)?
            .write(b"stop; config 0;")
            .map_err(Error::Io)?;
        // Stop hostside USB device.
        let result = board_ioctl_consts::ioctl_end(self.handle).map(|_| ());

        log::info!("VHF stopped!");

        result
    }

    /// Assumes the USB Machine has started.
    /// Gets the next index to read up to as given by ioctl
    #[inline(always)]
    pub fn ioctl_next(&self) -> Result<libc::c_int> {
        board_ioctl_consts::ioctl_read(self.handle)
    }

    /// Returns an iterable over VHF's buffer.
    pub fn iter<'a>(&'a self) -> VHFIter<'a> {
        VHFIter {
            map_reader: Rc::clone(&self.map_reader),
            engine_running: &self.engine_running,
            buffer_signal: &self.buffer_signal,
            buffer: &self.buffer,
            time_between_pages: self.time_between_pages,
            wake_mmap: &self.wake_mmap,
            total_pages_to_read: self.total_pages_to_read,
            windows_released: 0,
        }
    }
}

trait WakeMapReader {
    /// Wake MMapReader child thread.
    fn unpark_child(&self);
}

impl WakeMapReader for VHF {
    fn unpark_child(&self) {
        self.map_reader.thread().unpark()
    }
}

pub struct VHFIter<'a> {
    /// This is for calling the parent struct [VHF] solely for intention of being able to tell the
    /// child thread to park.
    map_reader: Rc<JoinHandle<Result<()>>>,
    /// Used to signal to [self::mmap_reader::MMapReader] has started, and to determine that child has stopped.
    engine_running: &'a Arc<AtomicBool>,
    /// Used to receive signal from [self::mmap_reader::MMapReader] that new pages have been placed into
    /// [self.buffer].
    buffer_signal: &'a Arc<Condvar>,
    /// buffer is a local mirror of Mmap that is intended for the likes of SlidingWindow
    /// [itertools::tuple_windows] and par_map, which has more Rust Semantics than reading straight
    /// out of a Mmap.
    buffer: &'a Arc<Mutex<Deque<MmapPage, DEQUE_CAP>>>,
    /// This is the amount of time between any two pages. Used for determining other timings.
    time_between_pages: Span,
    /// Expected time when to next wake up mmap_reader thread.
    wake_mmap: &'a Arc<RwLock<Instant>>,
    /// This is the total number of pages to be read by [self::MMapReader].
    total_pages_to_read: NonZeroUsize,
    /// Number of windows released to .iter() or par_iter() so far.
    windows_released: usize,
}

impl<'a> WakeMapReader for VHFIter<'a> {
    /// Wake MMapReader child thread.
    fn unpark_child(&self) {
        log::trace!("Unparking MmapReader thread");
        self.map_reader.thread().unpark()
    }
}

impl<'a> std::iter::Iterator for VHFIter<'a> {
    type Item = (usize, [MmapPage; VHF_MMAP_WINDOW_LEN]);

    // The idea: To ensure not having to manually drop any lifetimes (which could probably be
    // consumed by the LPF transformer or the FileWriter), it is easier for a SlidingWindow to
    // holdonto the Rc<RefCell<_>> instead. This way, dropping from the heap is automatically
    // managed by the reference counting of Rc<_>.
    // Next, we can dynamically increase the "prior" context window that is necessary for the LPF
    // transformer (or any other transformer), since LPF might often require data that precedes
    // the initial data point associated to the page of data. However, there probably is a need to
    // distinguish None for end of iterator versus None for no-prior data for start of stream
    // (transformers must implement the difference themselves).
    // Transformers in the steady state would not know where in the stream they are in, which may
    // quite likely be necessary to know which data points to skip (or not), and so, would also
    // want to take an index of the "body" page (i.e.: the first page that is not involved with
    // context).
    // The body page's index in the window has yet to be decided as being coordinated between the
    // Transform generator and this VHF, or if should be passed as a parameter.
    //
    // Note!: Buffer cannot be wrapped in a condvar, as it would block the next method. Condvar
    // wrapping is acceptable only if condvar notify occurs on average at least once per page
    // pushed onto the buffer.
    //
    // This method should be responsible for only moving the window forward by one.
    // ?: Anything that calls into VHF.next() should be using .step_by() before passing to the
    // transformer.
    fn next(&mut self) -> Option<Self::Item> {
        if self.windows_released > self.total_pages_to_read.into() {
            return None;
        }

        // Condvar needs a mutex guard for wait time out. We're using condvar "as a channel for now".
        // WARN: Mutex creation every time next method is called.
        let mg = Mutex::new(());

        let time_between_pages = self.time_between_pages.try_into().unwrap();

        'get_page: loop {
            // Return first if there are more elements in the buffer
            if let Ok(mut buf) = self.buffer.try_lock() {
                // if length >> num_items then assign to variable, pop left most, and return
                if buf.len() >= VHF_MMAP_WINDOW_LEN {
                    let arr = buf
                        .iter()
                        .take(VHF_MMAP_WINDOW_LEN)
                        .cloned()
                        .collect_array()
                        .unwrap();
                    let idx = self.windows_released;
                    self.windows_released += 1;
                    let _ = buf.pop_front();
                    return Some((idx, arr));
                }
            } else if self.buffer.is_poisoned() {
                panic!("MMapReader thread has panicked");
            }; // We do nothing even if buffer was not locked: Child thread might still be pushing
               // into it.

            // Early break - Engine is not running anymore for any reason (Thread panic perhaps?)
            if !self
                .engine_running
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                return None;
            };

            // No more elements in the buffer, and engine is running, we have to wake the thread.
            // We will wake up and fetch when either
            // 1. Condvar activated or
            // 2. We self check that the current instant exceeds the time as a last measure.

            'get_wake: loop {
                if let Ok(target_wakeup) = self.wake_mmap.read() {
                    let target_wakeup = *target_wakeup;

                    let now = Instant::now();
                    if now < target_wakeup {
                        let sleep_for = target_wakeup.saturating_duration_since(now);
                        thread::sleep(sleep_for);

                        // Wait 1 page of time for condvar
                        let timeout = self
                            .buffer_signal
                            .wait_timeout(mg.lock().unwrap(), time_between_pages)
                            .unwrap();
                        if !timeout.1.timed_out() {
                            // Forcibly try to get a page
                            log::trace!("Cond_var timedout");
                            continue 'get_page;
                        }
                        // The intended amount of thread::sleep has been performed.
                        // Fall through as if now > target_wakeup
                    }
                    self.unpark_child();
                    break 'get_wake;
                } else {
                    continue 'get_wake;
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Account for window being slightly different from number of pages being collected.
        let total: usize = self.total_pages_to_read.into();
        let lb = total.saturating_sub(self.windows_released);
        let lb = lb as usize;
        (lb, Some(lb + VHF_MMAP_WINDOW_LEN))
    }
}

#[cfg(test)]
mod test_v1_write;
#[cfg(test)]
mod test_vhf;
#[cfg(test)]
mod test_vhf_step_fold;
