pub mod repr;

use super::process::{
    consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN},
    pages::MmapPage,
};
use super::writer::WriteBlock;
use crate::{
    Result,
    parser::consts::M_OVERFLOW,
    types::{IQMTriplet, RawVHFWord},
};
use repr::Representation;
use serde::Serialize;
use std::{cmp::Ordering, hint::unreachable_unchecked, num::NonZeroU64, ops::Deref, sync::Arc};

#[derive(Clone)]
pub struct StreamFold {
    /// This the function that has to be applied to every chunked window from [super::VHF].next.
    pub func: Arc<dyn Fn(<super::VHFIter as Iterator>::Item) -> WriteBlock + Send + Sync>,
    /// This is the number of windows to step by each time prior to par_iter.
    pub step_by: usize,
    /// This is the number of windows to pad to the start.
    pub pad: usize,
    /// This is the operation performed.
    pub op: StreamFoldOp,
    /// This is a representation that aims to convey what was done in the fold. See [repr].
    ///
    /// For example,
    /// 1) Only m-rollover was tracked, and so effectively nothing was done:
    /// ```json
    /// { fold: [] }
    /// ```
    /// 2) two-pass decimation with different decimation factors and filter kernels.
    pub repr: Vec<Representation<f64>>,
}

impl PartialEq for StreamFold {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.func), Arc::as_ptr(&other.func))
            && self.step_by == other.step_by
            && self.pad == other.pad
    }
}

impl Eq for StreamFold {}

/// Determines the mode of operation on [super::VHF].next.
#[derive(Clone, PartialEq, Eq)]
pub enum StreamFoldOp {
    /// Identity Transform on Stream without index checking
    None,
    // Reduce,
    /// Quite literally the map in functional programming.
    ///
    /// If the enclosed Option is None, means that the map is *effectively* the same as
    /// [StreamFoldOp::None]. This is needed for [crate::runner::writer::V1Writer] and
    /// [crate::runner::writer::V1StdOut], which require that the phase and skip values are not
    /// altered in the fold process.
    /// If [Option::Some], consider the context of [super::Config::skip_num], StreamFold will lead
    /// to a decrease in number of elements between the FPGA and what is written to the file. This
    /// number summarizes the possuibly multiple steps performed by [StreamFold::func].
    Map(Option<NonZeroU64>),
}

impl std::fmt::Debug for StreamFold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamFold")
            .field("func", &"...")
            .field("step_by", &self.step_by)
            .field("pad", &self.pad)
            .field(
                "op",
                &match self.op {
                    StreamFoldOp::None => "none",
                    StreamFoldOp::Map(_) => "map",
                },
            )
            .finish()
    }
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
        fn overlapping_identity((idx, pages): <super::VHFIter as Iterator>::Item) -> WriteBlock {
            if idx == 0 {
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
                (idx, (a, b)): (usize, (&RawVHFWord, &RawVHFWord)),
            ) -> Option<(usize, i8)> {
                let IQMTriplet(_, _, a) = a.into();
                let IQMTriplet(_, _, b) = b.into();
                if a.abs_diff(b) >= M_OVERFLOW {
                    match b.cmp(&a) {
                        // The 2nd element of the window found to be less => overflow to negative
                        Ordering::Less => Some((idx, 1)),
                        // The 2nd element of the window found to be more => underflow to positive
                        Ordering::Greater => Some((idx, -1)),
                        // Safety: M_OVERFLOW check above.
                        Ordering::Equal => unsafe { unreachable_unchecked() },
                    }
                } else {
                    None
                }
            }

            use itertools::Itertools;
            if idx == 0 {
                // Let the 0th window be the first non-empty page's first element in the tuple 0th
                // and first. This ensures that the enumerate method's 0th index will be return the
                // 0 sign change.
                result.with_overflow_from_iter(
                    ([pages[PAGES_START].first().unwrap()])
                        .into_iter()
                        .chain(pages.iter().skip(PAGES_START).flat_map(Deref::deref))
                        .tuple_windows()
                        .enumerate()
                        .filter_map(idx_and_sign_for_filter_map),
                );
            } else {
                result.with_overflow_from_iter(
                    pages
                        .iter()
                        .flat_map(Deref::deref)
                        .tuple_windows()
                        .skip(PAGES_START * MMAP_PAGE_LEN - 1) // Skip all but last element of 0th page
                        .enumerate()
                        .filter_map(idx_and_sign_for_filter_map),
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

    /// Determine if the process of [self] creates any sort of decimation.
    /// Result because reduce would not make sense to call this.
    pub fn effective_decimation_factor(&self) -> Result<u64> {
        match self.op {
            StreamFoldOp::None => Ok(1),
            StreamFoldOp::Map(e) => Ok(e.map(|v| v.into()).unwrap_or(1)),
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
