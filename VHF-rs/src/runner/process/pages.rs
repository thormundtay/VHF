//! Map mmap fetched items into Heap-allocated chunks.

use super::consts::MMAP_PAGE_LEN;
// The individual elements as obtained from [super::board_ioctl_consts::ioctl_read].
use crate::types::RawVHFWord;
use crate::{Error, Result};
use std::sync::Arc;

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
}

/// One heap-allocated kernel page of 4KiB.  
/// This can be checked with `grep -ir pagesize /proc/self/smaps`.
/// This is used to allocate from VHF Memmap raw bytes onto the Heap.
pub type Page = Arc<[RawVHFWord; MMAP_PAGE_LEN]>;

/// Convenience methods for reaching into [MMapPage] or [Page].
pub(crate) trait PageInner {
    fn inner(&self) -> &[RawVHFWord];
}

impl PageInner for Page {
    fn inner(&self) -> &[RawVHFWord] {
        self.as_ref()
    }
}

impl PageInner for MmapPage {
    fn inner(&self) -> &[RawVHFWord] {
        match self {
            MmapPage::Empty => &[],
            MmapPage::Page(x) => x.inner(),
        }
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
