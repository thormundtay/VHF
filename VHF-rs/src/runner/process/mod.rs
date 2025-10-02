//! Interface with VHF, with things such as starting the VHF, reading out from it, closing it.

mod board_ioctl_consts;
pub(super) mod consts;
mod mmap_reader;
pub(super) mod pages;

use super::Config;
use super::config::BoardConfig;
use super::fold::StreamFoldFunction;
use crate::{Error, Result};
use consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN};
use heapless::Deque;
use itertools::Itertools;
use jiff::{Span, Zoned};
use mmap_reader::mmap_thread;
use mmap_rs::Mmap;
use nix::fcntl;
use pages::MmapPage;
use std::cell::RefCell;
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::{
    Arc, Condvar, Mutex, RwLock,
    atomic::{self, AtomicBool},
    mpsc::{Receiver, sync_channel},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use vhf_common::config_types::SamplingSpeed;

/// This is the size in bytes of the Mmap that is backed by the VHF device.
const MMAP_BYTES_LEN: usize = 1 << 22;
/// The size of the only continuous ring buffer that is [heapless::Deque].
pub(super) const DEQUE_CAP: usize = 256;

/// Logs messages as debug in #[cfg(test)], otherwise at their respective levels for #..not(test)
macro_rules! my_log {
    (error, $msg:expr $(, $($arg:tt)*)?) => {
        #[cfg(not(test))]
        log::error!($msg $(, $($arg)*)?);
        #[cfg(test)]
        log::debug!($msg $(, $($arg)*)?);
    };
    (warn, $msg:expr $(, $($arg:tt)*)?) => {
        #[cfg(not(test))]
        log::warn!($msg $(, $($arg)*)?);
        #[cfg(test)]
        log::debug!($msg $(, $($arg)*)?);
    };
    (info, $msg:expr $(, $($arg:tt)*)?) => {
        #[cfg(not(test))]
        log::info!($msg $(, $($arg)*)?);
        #[cfg(test)]
        log::debug!($msg $(, $($arg)*)?);
    };
}

/// Everything necessary to ensure the lifetime of pulling memory out from the VHF for its runtime
pub struct VHF<'a> {
    pub(super) configuration: BoardConfig<'a>,
    pub(super) handle: libc::c_int,
    pub(super) raw_handle: std::fs::File,
    /// map_reader contains the thread that is responsible for pulling elements out of the MMap
    /// into a [Self::buffer].
    /// More details is as given in [self::mmap_reader].
    pub(super) map_reader: Option<JoinHandle<Result<()>>>,
    /// Used to signal to [self::mmap_reader::MMapReader] has started, and to determine that child has stopped.
    pub(super) engine_running: Arc<AtomicBool>,
    /// Stopped invoked
    pub(super) vhf_stop: bool,
    /// Used to receive signal from [self::mmap_reader::MMapReader] that new pages have been placed into
    /// [Self::buffer].
    pub(super) buffer_signal: Arc<Condvar>,
    /// This is the channel used to receive from the child thread.
    pub(super) buffer_receive: Receiver<MmapPage>,
    /// buffer is a local mirror of Mmap that is intended for the likes of SlidingWindow
    /// [itertools::Itertools::tuple_windows] and par_map, which has more Rust Semantics than reading straight
    /// out of a Mmap.
    pub(super) buffer: Rc<RefCell<Deque<MmapPage, DEQUE_CAP>>>,
    /// This is the amount of time between any two pages. Used for determining other timings.
    pub(super) time_between_pages: Span,
    /// Expected time when to next wake up mmap_reader thread.
    pub(super) wake_mmap: Arc<RwLock<Instant>>,
    /// This is the total number of pages to be read by [Self::map_reader].
    pub(super) total_pages_to_read: NonZeroUsize,
}

impl<'a> VHF<'a> {
    /// Create a new instance of VHF control. The goal of [VHF] is to create all necessary control
    /// flow to read out of Mmap and to stream in the `impl Iterator for VHF` trait.
    /// # Arguments
    /// - config: [Config]
    ///   Configuration for running VHF.
    /// - params: [StreamFoldFunction]
    ///   This is to ensure that the same parameters are being used by the driving body and [VHF].
    pub fn new(config: &'a Config, params: &StreamFoldFunction) -> Result<Self> {
        let config: BoardConfig<'_> = config.build_board_config()?;
        let handle = Self::open_dev(
            config
                .board
                .clone()
                .into_os_string()
                .to_str()
                .ok_or(Error::ParseEmpty)?,
        )?; // TODO: OsStr -> &str validation should be done by Config

        assert_eq!(config.stream_fold_parameters(), params);

        let time_between_pages: Span = pages::time_between_pages_in_ns(config.speed)?;
        log::debug!("Time between pages = {time_between_pages}");

        let wake_mmap = Arc::new(RwLock::new(Instant::now()));

        // This will have to be changed as filtering etc means data points are not being passed to
        // file writer.
        let total_elements_to_read: NonZeroUsize =
            unsafe { NonZeroUsize::new(config.total_elements_to_read()).unwrap_unchecked() }
                .checked_mul(params.effective_decimation_factor())
                .ok_or(Error::ExcessData)?;
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
            (0..config.stream_fold.func.pad)
                .try_for_each(|_| buffer.push_back(MmapPage::Empty))
                .expect("Failed to push_back onto buffer.");
            Rc::new(RefCell::new(buffer))
        };
        let buffer_signal = Arc::new(Condvar::new()); // merge into buffer?
        let (buffer_producer, buffer_consumer) = sync_channel(DEQUE_CAP);

        let map_reader: JoinHandle<Result<()>> = {
            let readback = Self::readback_buffer(&raw_handle)?;
            let engine_running = engine_running.clone();
            let buffer_signal = buffer_signal.clone();
            let number_of_pages_between_stream_resume = (VHF_MMAP_WINDOW_LEN as i64 - 4) * 3;
            let time_between_stream_resume = (number_of_pages_between_stream_resume
                * time_between_pages)
                .try_into()
                .map_err(Error::Jiff)?;
            debug_assert!(number_of_pages_between_stream_resume < DEQUE_CAP as i64 / 2);
            log::debug!("MMapReader time_between_stream_resume = {time_between_stream_resume:?}");
            let next_collect_time = wake_mmap.clone();
            let streamfoldfunc = params.clone();
            thread::Builder::new()
                .name("mmap_reader".to_string())
                .spawn(move || {
                    mmap_thread(
                        readback,
                        engine_running,
                        buffer_signal,
                        buffer_producer,
                        &time_between_pages,
                        time_between_stream_resume,
                        next_collect_time,
                        total_pages_to_read,
                        handle,
                        &streamfoldfunc,
                    )
                })
                .map_err(Error::Io)
        }?;

        log::debug!("Readback buffer thread created");

        Ok(Self {
            configuration: config,
            handle,
            raw_handle,
            map_reader: Some(map_reader),
            engine_running,
            vhf_stop: false,
            buffer_signal,
            buffer_receive: buffer_consumer,
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
            // Use of variable to not optimize out.
            log::debug!("Prepopulating mmap summed to: {tmp}");
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
                .write(
                    format!("config 16; param {};", {
                        // Default is 0
                        let fc = config.filter_const.unwrap_or(0);
                        match config.speed {
                            SamplingSpeed::Low => (fc << 1) | 1,
                            SamplingSpeed::High => (fc << 1) & !1,
                        }
                    })
                    .as_bytes(),
                )
                .map_err(Error::Io)?; // filter_const + fast/slow
            buf_write
                .write(format!("config 1; param {};", config.skip_num).as_bytes())
                .map_err(Error::Io)?; // skips samples
            buf_write
                .write(format!("config 2; param {};", config.gain.unwrap_or(0)).as_bytes())
                .map_err(Error::Io)?; // Gain parameter
            buf_write.write(b"config 3; param 0;").map_err(Error::Io)?; // debug param = 0
            buf_write.write(b"skip; skip;").map_err(Error::Io).unwrap();
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
        #[cfg(not(feature = "clear-fifo"))]
        {
            my_log!(info, "VHF stop has been invoked.");
        }
        #[cfg(feature = "clear-fifo")]
        {
            log::debug!("VHF stop has been invoked.");
        }
        if self.vhf_stop {
            log::warn!("Parent thread found engine to have already been stopped.");
            return Err(Error::EngineStopped);
        }
        self.vhf_stop = true;

        if let Some(map) = self.map_reader.take() {
            if !map.is_finished() {
                log::warn!("MMapReader child thread not found to be exhausted.");
                self.map_reader = Some(map); // Place back into struct.
            } else if map.join().is_err() {
                log::warn!("MMapReader child thread found to have panicked.");
            } else {
                log::debug!("MMapReader child thread closed successfully.")
            }
        } else {
            log::warn!("MMapReader child thread not found. Was VHF already closed?");
        }

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
        let result = {
            #[cfg(not(test))]
            {
                board_ioctl_consts::ioctl_end(self.handle).map(|_| ())
            }
            #[cfg(test)]
            {
                // There is no need to perform ioctl_end in unit tests
                Ok(())
            }
        };

        #[cfg(not(feature = "clear-fifo"))]
        {
            my_log!(info, "VHF stopped!");
        }
        #[cfg(feature = "clear-fifo")]
        {
            log::debug!("VHF stopped!");
        }

        result
    }

    /// Returns an iterable over VHF's buffer.
    pub fn iter(&self) -> VHFIter<'_> {
        VHFIter {
            vhf_parent: self,
            engine_running: &self.engine_running,
            buffer_signal: &self.buffer_signal,
            buffer_receive: &self.buffer_receive,
            buffer: Rc::clone(&self.buffer),
            time_between_pages: self
                .time_between_pages
                .try_into()
                .expect("Failed to convert Span to Duration"),
            wake_mmap: &self.wake_mmap,
            total_pages_to_read: self.total_pages_to_read,
            windows_released: 0,
        }
    }

    /// Unpark MMapReader thread
    #[inline]
    fn unpark_child(&self) {
        self.map_reader.as_ref().unwrap().thread().unpark()
    }
}

impl Drop for VHF<'_> {
    fn drop(&mut self) {
        if !self.vhf_stop {
            log::debug!("VHF cleanup on drop has been invoked for us.");

            let stop_result = self.stop();
            if stop_result.is_err() {
                log::error!("VHF.stop had error during drop: {stop_result:?}");
            }
        }
    }
}

/// Immutable iterator of VHF pages.
///
/// This struct is created with the [VHF::iter] method on [VHF].  
/// Releases a window of VHF Pages with the [Self::next] method.
pub struct VHFIter<'a> {
    /// non-iter parent
    vhf_parent: &'a VHF<'a>,
    /// Used to signal to [self::mmap_reader::MMapReader] has started, and to determine that child has stopped.
    engine_running: &'a Arc<AtomicBool>,
    /// Used to receive signal from [self::mmap_reader::MMapReader] that new pages have been placed into
    /// [self.buffer].
    buffer_signal: &'a Arc<Condvar>,
    /// This is the channel used to receive from the child thread.
    buffer_receive: &'a Receiver<MmapPage>,
    /// buffer is a local mirror of Mmap that is intended for the likes of SlidingWindow
    /// [itertools::Itertools::tuple_windows] and par_map, which has more Rust Semantics than reading straight
    /// out of a Mmap.
    buffer: Rc<RefCell<Deque<MmapPage, DEQUE_CAP>>>,
    /// This is the amount of time between any two pages. Used for determining other timings.
    time_between_pages: Duration,
    /// Expected time when to next wake up mmap_reader thread.
    wake_mmap: &'a Arc<RwLock<Instant>>,
    /// This is the total number of pages to be read by [mmap_reader::MMapReader].
    total_pages_to_read: NonZeroUsize,
    /// Number of windows released to .iter() or par_iter() so far.
    windows_released: usize,
}

impl std::iter::Iterator for VHFIter<'_> {
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
            log::debug!("next yielding none: Released enough windows.");
            return None;
        }

        // Condvar needs a mutex guard for wait time out. We're using condvar "as a channel for now".
        // WARN: Mutex creation every time next method is called.
        let mg = Mutex::new(());

        'get_page: loop {
            // Try non-blocking fetch out from channel
            while let Ok(page) = self.buffer_receive.try_recv() {
                let result = self.buffer.borrow_mut().push_back(page);
                if result.is_err() {
                    log::error!("Pushing onto internal buffer without sufficient space.");
                    #[cfg(test)]
                    {
                        panic!("Please use a longer delay in the signal generator for the tests.");
                    }
                    // Discard failed to push page.
                }
            }

            // Return first if there are more elements in the buffer
            // if length >> num_items then assign to variable, pop left most, and return

            if self.buffer.borrow().len() >= VHF_MMAP_WINDOW_LEN {
                let arr = self
                    .buffer
                    .borrow()
                    .iter()
                    .take(VHF_MMAP_WINDOW_LEN)
                    .cloned()
                    .collect_array()
                    .unwrap();
                let idx = self.windows_released;
                self.windows_released += 1;
                let _ = self.buffer.borrow_mut().pop_front();
                return Some((idx, arr));
            }

            // Early break - Engine is not running anymore for any reason (Thread panic perhaps?)
            if !self
                .engine_running
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                log::debug!("next yielding none: engine stopped running.");
                return None;
            };

            // No more elements in the buffer, and engine is running, we have to wake the thread.
            // We will wake up and fetch when either
            // 1. Condvar activated or
            // 2. We self check that the current instant exceeds the time as a last measure.

            thread::sleep(self.time_between_pages);
            'get_wake: loop {
                if let Ok(target_wakeup) = self.wake_mmap.read() {
                    let target_wakeup = *target_wakeup;

                    let now = Instant::now();
                    if now < target_wakeup {
                        let sleep_for = target_wakeup.saturating_duration_since(now);
                        log::trace!("Sleeping within 'get_wake for {sleep_for:?}");
                        thread::sleep(sleep_for);

                        // Wait 1 page of time for condvar
                        let timeout = self
                            .buffer_signal
                            .wait_timeout(mg.lock().unwrap(), self.time_between_pages)
                            .unwrap();
                        if !timeout.1.timed_out() {
                            // Forcibly try to get a page
                            log::trace!("Cond_var timedout");
                            continue 'get_page;
                        }
                        // The intended amount of thread::sleep has been performed.
                        // Fall through as if now > target_wakeup
                    }
                    self.vhf_parent.unpark_child();
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
        (lb, Some(lb + VHF_MMAP_WINDOW_LEN))
    }
}
