//! Map mmap fetched items into Heap-allocated chunks.

/// The individual elements as obtained from [super::consts::usb_ioctl_read].
use crate::types::RawVHFWord;
use crate::{Error, Result};
use std::sync::Arc;

/// This is the number of [crate::types::RawVHFWord] in one (kernel-sized) page emitted from the
/// MMap onto the heap.
pub(super) const MMAP_PAGE_LEN: usize = 512;

/// All possible pages placed in to the buffer of [super::VHF].
#[derive(Clone)]
pub enum MmapPage {
    /// For the very beginning of the stream being pulled out from the MMap, there is no "previous"
    /// page before the current page, and so, any function that works on the window from
    /// [super::VHF::next] will have to be different.
    Empty,
    /// One kernel page worth of MMap data.
    // NOTE: The allocation between pages can be considered as being fragmented; in contrast to the
    // VecDeque.
    Page(Page),
    /// Denotes the MMapReader has reached the final page. Iterator trait for [super::VHF::next]
    /// should exit.
    End,
}

/// One heap-allocated kernel page of 4KiB.  
/// This can be checked with `grep -ir pagesize /proc/self/smaps`.
/// This is used to allocate from VHF Memmap raw bytes onto the Heap.
#[derive(Clone)]
#[repr(transparent)]
pub struct Page<const N: usize = MMAP_PAGE_LEN>(Arc<[RawVHFWord; N]>);

impl<const N: usize> Page<N> {
    pub fn new(x: Arc<[RawVHFWord; N]>) -> Self {
        Self(x)
    }
}

pub fn time_between_pages_in_ns(
    speed: &super::super::config::typedef::SamplingSpeed,
) -> Result<jiff::Span> {
    speed
        .in_ns()
        .checked_mul(MMAP_PAGE_LEN.try_into().unwrap())
        .map_err(Error::Jiff)
}
