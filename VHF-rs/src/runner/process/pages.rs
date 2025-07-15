//! Map mmap fetched items into Heap-allocated chunks.

use super::consts::MMAP_PAGE_LEN;
use crate::{Error, Result};
use std::{fmt::Debug, ops::Deref, sync::Arc};
// The individual elements as obtained from [super::board_ioctl_consts::ioctl_read].
use vhf_common::data_types::{IQMTriplet, RawVHFWord};

/// All possible pages placed in to the buffer of [super::VHF].
#[derive(Clone)]
pub enum MmapPage {
    /// For the very beginning of the stream being pulled out from the MMap, there is no "previous"
    /// page before the current page, and so, any function that works on the window from
    /// <[super::VHFIter] as Iterator>::next will have to be different.
    Empty,
    /// One kernel page worth of MMap data.
    // NOTE: The allocation between pages can be considered as being fragmented; in contrast to the
    // VecDeque.
    Page(Page),
    /// Denotes the MMapReader has reached the final page. [stream][super::VHFIter] should then exit.
    End,
}

/// One heap-allocated kernel page of 4KiB.  
/// This can be checked with `grep -ir pagesize /proc/self/smaps`.
/// This is used to allocate from VHF Memmap raw bytes onto the Heap.
pub type Page = Arc<[RawVHFWord; MMAP_PAGE_LEN]>;

impl Deref for MmapPage {
    type Target = [RawVHFWord];
    fn deref(&self) -> &Self::Target {
        match self {
            MmapPage::Empty => &[],
            MmapPage::Page(x) => (*x).as_slice(),
            MmapPage::End => &[],
        }
    }
}

impl Debug for MmapPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                MmapPage::Empty => "MmapPage::Empty".to_string(),
                MmapPage::Page(x) => {
                    let mut front = x.iter().take(2).map(IQMTriplet::from);
                    let mut back = x.iter().rev().take(2).map(IQMTriplet::from).rev();
                    format!(
                        "MmapPage::Page({:?}, {:?}, ..., {:?}, {:?})",
                        front.next().unwrap(),
                        front.next().unwrap(),
                        back.next().unwrap(),
                        back.next().unwrap()
                    )
                }
                MmapPage::End => "MmapPage::End".to_string(),
            }
        )
    }
}

pub fn time_between_pages_in_ns(
    speed: &vhf_common::config_types::SamplingSpeed,
) -> Result<jiff::Span> {
    speed
        .in_ns()
        .checked_mul(MMAP_PAGE_LEN.try_into().unwrap())
        .map_err(Error::Jiff)
}
