//! [MMapReader] aims to act as a "child class" for [super::VHF], but with the sole responsibility
//! of [mmap_rs::MMap] + ioctl management.
//! The intended entry point for [super::VHF] is to spawn [MMapReader] into a child thread through
//! the use of [mmap_thread].

use super::{consts::MMAP_PAGE_LEN, pages::MmapPage, MMAP_BYTES_LEN};
use crate::{Error, Result};
use jiff::Span;
use mmap_rs::Mmap;
use std::collections::VecDeque;
use std::hint::spin_loop;
use std::num::NonZeroU64;
use std::sync::{
    atomic::{self, AtomicBool},
    Arc, Condvar, Mutex, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

/// Bottom 12 bytes of [super::board_ioctl_consts::ioctl_read] should be zero'd to align to [MMapPage::Page].
pub const ALIGN_VHF_OUTPUT_TO_PAGES: usize = 9 + 3;

pub(super) struct MMapReader {
    // FileHandle associated to mmap is needed to ioctl_next;
    handle: libc::c_int,
    // Ownership of [Mmap] throughout the lifetime of the entire program should be limited to this
    // struct.
    mmap: Mmap,
    // Used to determine that parents has started, and to signal back to parent thread that stop
    // can be called.
    // NOTE: Reading into this if infinite stream to know to stop?
    engine_running: Arc<AtomicBool>, // Suboptimal
    /// Used to signal back to parent that a page hasbeen placedinto [self.transfer_buffer].
    transfer_buffer_signal: Arc<Condvar>,
    /// This is the means by which MMapReader passes pages back to [VHF] for VHF to act as an
    /// iterator.
    // Strongly note that MMapPages are therefore fragmented with respect to each other, but we eat
    // this cost first.
    transfer_buffer: Arc<Mutex<VecDeque<MmapPage>>>,
    /// This is the tfb32 value from the last ioctl.
    last_tfb32: libc::c_int,
    /// This is the last index being read from.
    prev_bytes: usize,
    /// Time for a single page.
    page_duration: Duration,
    /// Time between stream wakeups
    stream_pause: Duration,
    /// Maximal amount of time alloweable waiting for parent thread to unpark before self unpark.
    loop_timeout: Duration,
    /// Time after this is when we expect to start collecting the next set of pages.
    next_collect_time: Arc<RwLock<Instant>>,
    /// Number of VHF Pages to read. 0 for an infinite amount.
    total_pages: NonZeroU64,
    /// Number of pages thus far.
    collected_pages: u64,
}

impl MMapReader {
    pub(super) fn new(
        mmap: Mmap,
        engine_running: Arc<AtomicBool>,
        transfer_buffer_signal: Arc<Condvar>,
        transfer_buffer: Arc<Mutex<VecDeque<MmapPage>>>,
        time_between_mmap_page: &Span,
        time_between_stream_resume: Duration,
        next_collect_time: Arc<RwLock<Instant>>,
        total_pages: NonZeroU64,
        handle: libc::c_int,
    ) -> Result<Self> {
        let loop_timeout: Duration = (*time_between_mmap_page
            * (4 * super::VHF_MMAP_WINDOW_LEN)
                .max(super::DEQUE_CAP / 2)
                .try_into()
                .unwrap())
        .try_into()
        .map_err(Error::Jiff)?;
        Ok(Self {
            handle,
            mmap,
            engine_running,
            transfer_buffer_signal,
            transfer_buffer,
            last_tfb32: 0,
            prev_bytes: 0,
            page_duration: (*time_between_mmap_page).try_into().map_err(Error::Jiff)?,
            stream_pause: time_between_stream_resume,
            loop_timeout,
            next_collect_time,
            total_pages,
            collected_pages: 0,
        })
    }

    /// Assumes the USB Machine has started.
    /// Gets the next index to read up to as given by ioctl
    #[inline(always)]
    pub fn ioctl_next(&self) -> Result<libc::c_int> {
        super::board_ioctl_consts::ioctl_read(self.handle)
    }

    /// With the previous (rounded) bytes to current (rounded) byes, create a lazy iterator for
    /// pushing onto buffer.  
    /// Rounding done must be in accordance with [MMAP_PAGE_LEN]. Note that each [crate::types::RawVHFWord] is 8 bytes.
    fn get_mmap_iter<'a>(&'a self, prev: usize, next: usize) -> impl Iterator<Item = &'a u64> {
        use bytemuck::try_cast_slice;
        // Bytes rounded to page length should have
        debug_assert!(prev % (8 * MMAP_PAGE_LEN) == 0);
        debug_assert!(next % (8 * MMAP_PAGE_LEN) == 0);
        if prev < next {
            try_cast_slice(&self.mmap[prev..next])
                .unwrap()
                .into_iter()
                .chain(try_cast_slice(&self.mmap[..0]).unwrap())
        } else {
            try_cast_slice(&self.mmap[prev..MMAP_BYTES_LEN])
                .unwrap()
                .into_iter()
                .chain(try_cast_slice(&self.mmap[..next]).unwrap())
        }
    }

    /// This converts Mmap u8s into VHFPages which are placed into [self.buffer].
    fn stream(&mut self) -> Result<()> {
        let mut next_bytes;
        let mut time_after_mmap_fetch;
        loop {
            // If the current time has exceed the next expected fetch time, then proceed, else sleep.
            thread::park_timeout(self.loop_timeout); // May apparently spuriously wake.

            // Pull out from Mmap and place into heap
            'next_mmap: loop {
                let next = self.ioctl_next()?;
                if next < 0 {
                    return Err(Error::ioctl_call("Negative next value received."));
                }
                if next.wrapping_sub(self.last_tfb32) <= (1 << ALIGN_VHF_OUTPUT_TO_PAGES) {
                    log::debug!("tried getting next before having more than a page of data...");
                    log::debug!("last_tfb32 = {}, next = {}", self.last_tfb32, next);
                    thread::park_timeout(self.page_duration);
                    continue;
                }

                // Enough pages have accumulated.
                let offset = (next as usize % MMAP_BYTES_LEN) >> ALIGN_VHF_OUTPUT_TO_PAGES
                    << ALIGN_VHF_OUTPUT_TO_PAGES;
                self.last_tfb32 = next;
                next_bytes = offset;
                time_after_mmap_fetch = Instant::now();
                break 'next_mmap;
            }

            // We deliberately do not fetch for a new value of ioctl whiles trying to get a
            // mutex lock. (We will consider updating in the future if the try_lock takes too
            // long.)
            'push_back: loop {
                match self.transfer_buffer.try_lock() {
                    Err(_) => spin_loop(),
                    Ok(mut inner) => {
                        use itertools::Itertools;

                        let mut num_pages = 0;

                        self.get_mmap_iter(self.prev_bytes, next_bytes)
                            .chunks(MMAP_PAGE_LEN)
                            .into_iter()
                            .map(|x| x.copied().collect_array().unwrap())
                            .map(Arc::new)
                            .map(MmapPage::Page)
                            .for_each(|x| {
                                num_pages += 1;
                                (*inner).push_back(x);
                            });

                        // Update counter
                        debug_assert_eq!(
                            num_pages as usize,
                            (next_bytes - self.prev_bytes)
                                .min(MMAP_BYTES_LEN - next_bytes - self.prev_bytes)
                        );

                        self.collected_pages += num_pages;
                        self.prev_bytes = next_bytes;

                        // Signal back to parent thread that pages have been placed in.
                        self.transfer_buffer_signal.notify_all();

                        // Set next wake time
                        {
                            let mut to_write = self.next_collect_time.write().unwrap();
                            *to_write = time_after_mmap_fetch + self.stream_pause;
                        }

                        break 'push_back;
                    }
                };
            }

            // If number of pages read has exceeded break
            if self.collected_pages >= self.total_pages.into() {
                break;
            }
        }

        Ok(())
    }

    /// Cleaning up before thread exits.
    fn close(&self) -> Result<()> {
        self.engine_running.store(false, atomic::Ordering::Relaxed);
        Ok(())
    }
}

impl core::ops::Drop for MMapReader {
    fn drop(&mut self) {
        match thread::panicking() {
            true => log::error!("MmapReader panicking!"),
            false => log::info!("MMapReader has been dropped."),
        }
    }
}

/// Used as the child thread of [super::VHF] at driving [MMapReader].
pub(super) fn mmap_thread(
    mmap: Mmap,
    engine: Arc<AtomicBool>,
    buffer_signal: Arc<Condvar>,
    buffer: Arc<Mutex<VecDeque<MmapPage>>>,
    time_between_mmap_page: &Span,
    time_between_stream_resume: Duration,
    next_collect_time: Arc<RwLock<Instant>>,
    total_pages: NonZeroU64,
    handle: libc::c_int,
) -> Result<()> {
    let mut mmap_reader = MMapReader::new(
        mmap,
        engine,
        buffer_signal,
        buffer,
        time_between_mmap_page,
        time_between_stream_resume,
        next_collect_time,
        total_pages,
        handle,
    )?;

    // Block until parent has started.
    while !mmap_reader.engine_running.load(atomic::Ordering::Acquire) {
        thread::park();
    }

    // Main drive: Place into Buffer.
    mmap_reader.stream()?;

    // Cleanup
    mmap_reader.close()?;

    Ok(())
}
