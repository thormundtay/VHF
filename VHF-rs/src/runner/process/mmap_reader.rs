//! [MMapReader] aims to act as a "child class" for [super::VHF], but with the sole responsibility
//! of [mmap_rs::MMap] + ioctl management.

use super::{
    pages::{MmapPage, MMAP_PAGE_LEN},
    MMAP_BYTES_LEN,
};
use crate::{Error, Result};
use mmap_rs::Mmap;
use std::collections::VecDeque;
use std::hint::spin_loop;
use std::sync::{
    atomic::{self, AtomicBool},
    Arc, Mutex,
};
use std::thread;

/// Bottom 12 bytes should be zero'd to align to [MMapPage::Page].
const ALIGN_PAGES: usize = 9 + 3;

pub(super) struct MMapReader {
    // FileHandle associated to mmap is needed to ioctl_next;
    handle: libc::c_int,
    // Ownership of [Mmap] throughout the lifetime of the entire program should be limited to this
    // struct.
    mmap: Mmap,
    /// This is the means by which MMapReader passes pages back to [VHF] for VHF to act as an
    /// iterator.
    // Strongly note that MMapPages are therefore fragmented with respect to each other, but we eat
    // this cost first.
    transfer_buffer: Arc<Mutex<VecDeque<MmapPage>>>,
    /// This is the tfb32 value from the last ioctl.
    last_tfb32: libc::c_int,
    /// This is the last index being read from.
    prev_bytes: usize,
}

impl MMapReader {
    pub(super) fn new(
        mmap: Mmap,
        transfer_buffer: Arc<Mutex<VecDeque<MmapPage>>>,
        handle: libc::c_int,
    ) -> Result<Self> {
        Ok(Self {
            handle,
            mmap,
            transfer_buffer,
            last_tfb32: 0,
            prev_bytes: 0,
        })
    }

    /// Assumes the USB Machine has started.
    /// Gets the next index to read up to as given by ioctl
    #[inline(always)]
    pub fn ioctl_next(&self) -> Result<libc::c_int> {
        super::consts::ioctl_read(self.handle)
    }

    /// With the previous (rounded) bytes to current (rounded) byes, create a lazy iterator for
    /// pushing onto buffer.  
    /// Rounding done must be in accordance with [MMAP_PAGE_LEN]. Note that each [crate::types::RawVHFWord] is 8 bytes.
    fn get_mmap_iter<'a>(&'a self, prev: usize, next: usize) -> impl Iterator<Item = &'a u8> {
        // Bytes rounded to page length should have
        debug_assert!(prev % (8 * MMAP_PAGE_LEN) == 0);
        debug_assert!(next % (8 * MMAP_PAGE_LEN) == 0);
        if prev < next {
            self.mmap
                .iter()
                .skip(prev)
                .take(next - prev)
                .chain([0; 0].iter().take(0))
        } else {
            self.mmap
                .iter()
                .skip(prev)
                .take(MMAP_BYTES_LEN - prev)
                .chain(self.mmap.iter().take(next))
        }
    }

    /// This converts Mmap u8s into VHFPages which are placed into [self.buffer].
    fn stream(&mut self) -> Result<()> {
        let mut next_bytes;
        loop {
            // Pull out from Mmap and place into heap
            'next_mmap: loop {
                let next = self.ioctl_next()?;
                if next < 0 {
                    return Err(Error::ioctl_call("Negative next value received."));
                }
                if next.wrapping_sub(self.last_tfb32) <= (1 << ALIGN_PAGES) {
                    continue;
                }

                // Enough pages have accumulated.
                let offset = (next as usize % MMAP_BYTES_LEN) >> ALIGN_PAGES << ALIGN_PAGES;
                self.last_tfb32 = next;
                next_bytes = offset;
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
                        self.get_mmap_iter(self.prev_bytes, next_bytes)
                            .chunks(8)
                            .into_iter()
                            .map(|x| -> u64 {
                                let x: [u8; 8] = unsafe {
                                    x.into_iter().copied().collect_array().unwrap_unchecked()
                                };
                                u64::from_ne_bytes(x)
                            })
                            .chunks(MMAP_PAGE_LEN)
                            .into_iter()
                            .map(|x| -> [u64; MMAP_PAGE_LEN] {
                                let x: [u64; MMAP_PAGE_LEN] =
                                    unsafe { x.into_iter().collect_array().unwrap_unchecked() };
                                x
                            })
                            .map(Arc::new)
                            .map(super::pages::Page::new)
                            .map(MmapPage::Page)
                            .for_each(|x| (*inner).push_back(x));
                        break 'push_back;
                    }
                };
            }

            // Update counter
            self.prev_bytes = next_bytes;
        }

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
