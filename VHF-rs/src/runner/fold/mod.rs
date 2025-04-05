use super::process::consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN};
use super::writer::WriteBlock;
use crate::Result;
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
}
