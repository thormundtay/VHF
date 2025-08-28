//! This module is for all functionality pertaining to the folding - (map/reduce) of the VHF
//! stream.
//!
//! The two primary concerns of this module are:
//! 1. Human readability of folding, as expressed in file headers.
//! 2. Mathematical functions invoked for the fold.

mod func;
pub use func::StreamFoldFunction;
pub use func::{MapArg, StreamFoldOp};
pub mod repr;
pub use repr::StreamFoldRepr;

use super::process::{
    consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN},
    pages::MmapPage,
};
use super::writer::WriteBlock;
use crate::parser::consts::M_OVERFLOW;
use serde::Serialize;
use std::{cmp::Ordering, hint::unreachable_unchecked, ops::Deref, sync::Arc};
use vhf_common::data_types::{IQMTriplet, MOverflowRaw, RawVHFWord};

/// This contains the necessary information that is then delegated to both file writing and
/// "in-flight processing".
#[derive(Clone)]
pub struct StreamFold {
    // TODO
}

impl StreamFold {
    /// This is the Identity transform without any roll-over checking.
    pub fn none_default() -> Self {
        let identity = |(_, pages): <super::VHFIter as Iterator>::Item| {
            let data = {
                let mut data = Vec::with_capacity(VHF_MMAP_WINDOW_LEN * MMAP_PAGE_LEN);
                pages.into_iter().for_each(|p| data.extend(p.deref()));
                data
            };

            WriteBlock::new(data)
        };

        StreamFold {
            func: Arc::new(identity),
            step_by: VHF_MMAP_WINDOW_LEN,
            pad: 0,
            op: StreamFoldOp::None,
            repr: Vec::new(),
        }
    }

    //// This is the Identity transform with roll-over checking.
    pub fn identity_default() -> Self {
        /// Offset into each window in which all pages prior can be thought of as lookbacks into
        /// previous windows.
        const PAGES_START: usize = 1;

        // The "transform/map" as done by this function requires a "lookback" into the previous
        // word to determine if a rollover has occurred. As such, the 0th element has to be chosen
        // from the idx-1th page to ensure that the 0th window returns a sign of 0 change for the
        // 0th element in the stream.
        fn overlapping_identity(
            (vhf_iter_idx, pages): <super::VHFIter as Iterator>::Item,
        ) -> WriteBlock {
            if vhf_iter_idx == 0 {
                debug_assert!(matches!(pages[0], MmapPage::Empty));
                debug_assert!(matches!(pages[1], MmapPage::Page(_)));
            } else {
                debug_assert!(matches!(pages[0], MmapPage::Page(_)));
                debug_assert!(matches!(pages[1], MmapPage::Page(_)));
            };

            let data_iter = pages
                .iter()
                .skip(PAGES_START)
                .flat_map(Deref::deref)
                .copied();

            let mut result = WriteBlock::new_from_iter(data_iter);

            fn idx_and_sign_for_filter_map(
                (element_idx, (a, b)): (usize, (&RawVHFWord, &RawVHFWord)),
                vhf_iter_idx: usize,
            ) -> Option<MOverflowRaw> {
                let IQMTriplet(_, _, a) = a.into();
                let IQMTriplet(_, _, b) = b.into();
                if a.abs_diff(b) >= M_OVERFLOW {
                    // It takes 200k years to generate
                    // u64 elements even if there was no USB2.0 throttling; so it is safe to encode
                    // the index of RawVHFWord by absolute index relative to first FPGA word.
                    let offset = vhf_iter_idx.checked_mul(MMAP_PAGE_LEN).unwrap();

                    match b.cmp(&a) {
                        // The 2nd element of the window found to be less => overflow to negative
                        Ordering::Less => Some(MOverflowRaw(element_idx + offset, 1)),
                        // The 2nd element of the window found to be more => underflow to positive
                        Ordering::Greater => Some(MOverflowRaw(element_idx + offset, -1)),
                        // Safety: M_OVERFLOW check above.
                        Ordering::Equal => unsafe { unreachable_unchecked() },
                    }
                } else {
                    None
                }
            }

            use itertools::Itertools;
            if vhf_iter_idx == 0 {
                // Let the 0th window be the first non-empty page's first element in the tuple 0th
                // and first. This ensures that the enumerate method's 0th index will be return the
                // 0 sign change.
                result.with_overflow_from_iter(
                    ([pages[PAGES_START].first().unwrap()])
                        .into_iter()
                        .chain(pages.iter().skip(PAGES_START).flat_map(Deref::deref))
                        .tuple_windows()
                        .enumerate()
                        .filter_map(|w| idx_and_sign_for_filter_map(w, vhf_iter_idx)),
                );
            } else {
                result.with_overflow_from_iter(
                    pages
                        .iter()
                        .flat_map(Deref::deref)
                        .tuple_windows()
                        .skip(PAGES_START * MMAP_PAGE_LEN - 1) // Skip all but last element of 0th page
                        .enumerate()
                        .filter_map(|w| idx_and_sign_for_filter_map(w, vhf_iter_idx)),
                )
            };

            result
        }

        StreamFold {
            func: Arc::new(overlapping_identity),
            step_by: VHF_MMAP_WINDOW_LEN - PAGES_START,
            pad: PAGES_START,
            op: StreamFoldOp::Map(None),
            repr: Vec::new(),
        }
    }
}

impl Serialize for StreamFold {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Rather than specifying #[serde(skip_deserializing)] for all but self.repr.
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("StreamFold", 1)?;
        s.serialize_field("fold", &self.repr)?;
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarities() {
        let first = StreamFold::none_default();
        let second = StreamFold::identity_default();
        assert!(first == first);
        assert!(second == second);
        assert!(first != second);
    }
}
