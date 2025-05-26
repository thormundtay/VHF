//! [MMapReader] aims to act as a "child class" for [super::VHF], but with the sole responsibility
//! of [mmap_rs::Mmap] + ioctl management.
//! The intended entry point for [super::VHF] is to spawn [MMapReader] into a child thread through
//! the use of [mmap_thread].

use super::super::fold::StreamFold;
use super::{DEQUE_CAP, MMAP_BYTES_LEN, consts::MMAP_PAGE_LEN, pages::MmapPage};
use crate::{Error, Result};
use heapless::Deque;
use jiff::Span;
use mmap_rs::Mmap;
use std::hint::spin_loop;
use std::num::NonZeroUsize;
use std::sync::{
    Arc, Condvar, Mutex, RwLock,
    atomic::{self, AtomicBool},
};
use std::thread;
use std::time::{Duration, Instant};

/// Bottom 12 bytes of [super::board_ioctl_consts::ioctl_read] should be zero'd to align to [MmapPage::Page].
pub const ALIGN_VHF_OUTPUT_TO_PAGES: usize = 9 + 3;

#[derive(Debug)]
pub(super) struct MMapReader {
    // FileHandle associated to mmap is needed to ioctl_next;
    handle: libc::c_int,
    // Ownership of [self::Mmap] throughout the lifetime of the entire program should be limited to this
    // struct.
    mmap: Mmap,
    // Used to determine that parents has started, and to signal back to parent thread that stop
    // can be called.
    // NOTE: Reading into this if infinite stream to know to stop?
    engine_running: Arc<AtomicBool>, // Suboptimal
    /// Used to signal back to parent that a page hasbeen placedinto [self.transfer_buffer].
    transfer_buffer_signal: Arc<Condvar>,
    /// This is the means by which MMapReader passes pages back to [super::VHF] for VHF to act as an
    /// iterator.
    // Strongly note that MMapPages are therefore fragmented with respect to each other, but we eat
    // this cost first.
    transfer_buffer: Arc<Mutex<Deque<MmapPage, DEQUE_CAP>>>,
    /// This is the tfb32 value from the last ioctl.
    last_tfb32: libc::c_int,
    /// This is the last index being read from.
    prev_bytes: usize,
    /// Time for a single page.
    page_duration: Duration,
    /// Time between [stream][super::VHFIter] [`next`](../../struct.VHFIter.html#impl-Iterator-for-VHFIter<'_>) method wakeups
    stream_pause: Duration,
    /// Maximal amount of time alloweable waiting for parent thread to unpark before self unpark.
    loop_timeout: Duration,
    /// Time after this is when we expect to start collecting the next set of pages.
    next_collect_time: Arc<RwLock<Instant>>,
    /// Number of VHF Pages to read. 0 for an infinite amount.
    total_pages: NonZeroUsize,
    /// Number of pages thus far.
    collected_pages: usize,
    /// StreamFold specified parameter
    step_by: usize,
    /// StreamFold specified parameter
    stream_pad: usize,
}

impl MMapReader {
    pub(super) fn new(
        mmap: Mmap,
        engine_running: Arc<AtomicBool>,
        transfer_buffer_signal: Arc<Condvar>,
        transfer_buffer: Arc<Mutex<Deque<MmapPage, DEQUE_CAP>>>,
        time_between_mmap_page: &Span,
        time_between_stream_resume: Duration,
        next_collect_time: Arc<RwLock<Instant>>,
        total_pages: NonZeroUsize,
        handle: libc::c_int,
        streamfold: &StreamFold,
    ) -> Result<Self> {
        let loop_timeout: Duration = (*time_between_mmap_page
            * (4 * super::VHF_MMAP_WINDOW_LEN)
                .max(super::DEQUE_CAP / 2)
                .try_into()
                .unwrap())
        .try_into()
        .map_err(Error::Jiff)?;
        log::debug!("mmap_reader thread loop_timeout = {:?}", loop_timeout);

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
            step_by: streamfold.step_by(),
            stream_pad: streamfold.pad(),
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
    fn get_mmap_iter(&self, prev: usize, next: usize) -> impl Iterator<Item = &'_ u64> {
        use bytemuck::try_cast_slice;
        // Bytes rounded to page length should have
        debug_assert!(prev % (8 * MMAP_PAGE_LEN) == 0);
        debug_assert!(next % (8 * MMAP_PAGE_LEN) == 0);
        if prev < next {
            try_cast_slice(&self.mmap[prev..next])
                .unwrap()
                .iter()
                .chain(try_cast_slice(&self.mmap[..0]).unwrap())
        } else {
            try_cast_slice(&self.mmap[prev..MMAP_BYTES_LEN])
                .unwrap()
                .iter()
                .chain(try_cast_slice(&self.mmap[..next]).unwrap())
        }
    }

    /// This converts Mmap u8s into VHFPages which are placed into [self.buffer].
    /// # Errors
    /// [super::VHF::ioctl_next] yields Err or has negative value.
    fn stream(&mut self) -> Result<()> {
        let mut next_bytes;
        loop {
            // If the current time has exceed the next expected fetch time, then proceed, else sleep.
            log::trace!("mmap_reader thread park");
            thread::park_timeout(self.loop_timeout); // May apparently spuriously wake.
            log::trace!("mmap_reader thread unpark");

            // Pull out from Mmap and place into heap
            'next_mmap: loop {
                let next = self.ioctl_next()?;
                if next < 0 {
                    log::error!("Received negative next value.");
                    return Err(Error::ioctl_call("Negative next value received."));
                }
                if next.wrapping_sub(self.last_tfb32) & i32::MAX <= (1 << ALIGN_VHF_OUTPUT_TO_PAGES)
                {
                    thread::park_timeout(self.page_duration);
                    continue 'next_mmap;
                };
                if next.wrapping_sub(self.last_tfb32) & i32::MAX > (MMAP_BYTES_LEN as i32) {
                    log::error!("Circular buffer has already been overwritten!");
                    return Err(Error::InternalInconsistency);
                };

                // Enough pages have accumulated.
                let offset = (next as usize % MMAP_BYTES_LEN) >> ALIGN_VHF_OUTPUT_TO_PAGES
                    << ALIGN_VHF_OUTPUT_TO_PAGES;
                self.last_tfb32 = next;
                next_bytes = offset;
                let time_after_mmap_fetch = Instant::now();

                // Set next wake time, because pushing onto buffer does take a while.
                {
                    let mut to_write = self.next_collect_time.write().unwrap();
                    *to_write = time_after_mmap_fetch + self.stream_pause;
                }

                break 'next_mmap;
            }

            // We deliberately do not fetch for a new value of ioctl whiles trying to get a
            // mutex lock. (We will consider updating in the future if the try_lock takes too
            // long.)
            'push_back: loop {
                match self.transfer_buffer.try_lock() {
                    Err(_) => {
                        log::trace!("could not get buffer for pushing onto page");
                        spin_loop(); // This is a no-op on x86;
                    }
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
                                (*inner).push_back(x).expect("Failed to push back.");
                            });

                        // Update counter
                        self.collected_pages += num_pages;
                        self.prev_bytes = next_bytes;

                        // Signal back to parent thread that pages have been placed in.
                        self.transfer_buffer_signal.notify_all();

                        // Set next wake time again
                        {
                            let mut to_write = self.next_collect_time.write().unwrap();
                            *to_write = Instant::now() + self.stream_pause;
                        }

                        break 'push_back;
                    }
                };
            }

            // If number of pages read has exceeded break
            if self.collected_pages >= self.total_pages.into() {
                log::info!("MMapReader has collected pages >= total pages.");
                // Empty pad so that iterator can pull out final window.
                self.pad_end()?;
                break;
            }
        }

        Ok(())
    }

    /// Knowing the number of pages being stepped by, left-padded and number of pages collected
    /// thus far, one can determine the number of [super::pages::MmapPage::Empty] one needs to pad
    /// on the right by. This function then determines how many of such empty pages one requires.
    #[inline(always)]
    fn pad_end_remaining(&self) -> usize {
        // (a-2b) - (x mod (a-b)); where
        // a = VHF_MMAP_WINDOW_LEN
        // b = overlap
        let a = super::VHF_MMAP_WINDOW_LEN;
        let b = self.stream_pad;
        debug_assert!(a >= 2 * b);

        (a - 2 * b)
            .checked_sub(self.collected_pages.rem_euclid(self.step_by))
            .unwrap()
    }

    /// Because of .take_every(), we need to put in an appropriate number of blank pages at the end
    /// so that the final next() method can pull out all empty windows.
    fn pad_end(&self) -> Result<()> {
        let pad = self.pad_end_remaining();
        log::debug!("pad_end called with {} MMapPage::End to pad with", pad);
        'push_back: loop {
            match self.transfer_buffer.try_lock() {
                Err(_) => spin_loop(),
                Ok(mut inner) => {
                    (0..pad)
                        .for_each(|_| inner.push_back(MmapPage::End).expect("Failed to push_back"));
                    break 'push_back;
                }
            };
        }
        Ok(())
    }

    /// Cleaning up before thread exits.
    fn close(&self) -> Result<()> {
        log::info!("MMapReader has been invoked to be closed");
        self.engine_running.store(false, atomic::Ordering::Relaxed);
        Ok(())
    }
}

impl core::ops::Drop for MMapReader {
    fn drop(&mut self) {
        match thread::panicking() {
            true => {
                log::error!(
                    "MMapReader panicking! Had pushed {} pages.",
                    self.collected_pages
                );
                log::error!("MMapReader = {self:?}");
                log::error!("Forcing cleanup.");
                let _ = self.close();
            }
            false => log::info!("MMapReader has been dropped."),
        }
    }
}

/// Used as the child thread of [super::VHF] at driving [MMapReader].
pub(super) fn mmap_thread(
    mmap: Mmap,
    engine: Arc<AtomicBool>,
    buffer_signal: Arc<Condvar>,
    buffer: Arc<Mutex<Deque<MmapPage, DEQUE_CAP>>>,
    time_between_mmap_page: &Span,
    time_between_stream_resume: Duration,
    next_collect_time: Arc<RwLock<Instant>>,
    total_pages: NonZeroUsize,
    handle: libc::c_int,
    streamfold: &StreamFold,
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
        streamfold,
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
