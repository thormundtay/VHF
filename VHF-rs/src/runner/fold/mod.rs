use super::process::{
    consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN},
    pages::MmapPage,
};
use super::writer::WriteBlock;
use crate::{
    parser::consts::M_OVERFLOW,
    types::{IQMTriplet, RawVHFWord},
    Result,
};
use std::{cmp::Ordering, ops::Deref, rc::Rc};

#[derive(Clone)]
pub(super) struct StreamFoldParameters {
    /// This the function that has to be applied to every chunked window from [super::VHF].next.
    func: Rc<dyn Fn(<super::VHF as Iterator>::Item) -> WriteBlock>,
    /// This is the number of windows to step by each time prior to par_iter.
    step_by: usize,
}

/// Determines the mode of operation on [super::VHF].next.
#[derive(Clone)]
pub(super) enum StreamFold {
    /// Identity Transform on Stream without index checking
    None(StreamFoldParameters),
    // Reduce,
    /// Quite literally the map in functional programming.
    Map(StreamFoldParameters),
}

impl std::fmt::Debug for StreamFold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                StreamFold::None(_) => "none",
                StreamFold::Map(_) => "map",
            }
        )
    }
}

impl StreamFold {
    /// This is the Identity transform without any roll-over checking.
    pub(in crate::runner) fn none_default() -> StreamFold {
        let identity = |(_, pages): <super::VHF as Iterator>::Item| {
            let data = {
                let mut data = Vec::with_capacity(VHF_MMAP_WINDOW_LEN * MMAP_PAGE_LEN);
                pages.into_iter().for_each(|p| data.extend(p.deref()));
                data
            };

            WriteBlock::new(data)
        };

        StreamFold::None(StreamFoldParameters {
            func: Rc::new(identity),
            step_by: VHF_MMAP_WINDOW_LEN,
        })
    }

    //// This is the Identity transform with roll-over checking.
    pub(in crate::runner) fn identity_default() -> StreamFold {
        /// Offset into each window in which all pages prior can be thought of as lookbacks into
        /// previous windows.
        const PAGES_START: usize = 1;

        // The "transform/map" as done by this function requires a "lookback" into the previous
        // word to determine if a rollover has occurred. As such, the 0th element has to be chosen
        // from the idx-1th page to ensure that the 0th window returns a sign of 0 change for the
        // 0th element in the stream.
        fn overlapping_identity((idx, pages): <super::VHF as Iterator>::Item) -> WriteBlock {
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
                        Ordering::Equal => unreachable!(),
                    }
                } else {
                    Some((idx, 0))
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

        StreamFold::Map(StreamFoldParameters {
            func: Rc::new(overlapping_identity),
            step_by: VHF_MMAP_WINDOW_LEN - PAGES_START,
        })
    }
}
