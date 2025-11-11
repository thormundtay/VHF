//! This is the location where code should be placed for folding of [super::super::VHFIter] stream
//! pages.

use super::super::VHFIter;
use super::super::writer::WriteBlock;
use crate::{Error, Result};
use std::{num::NonZeroUsize, sync::Arc};

/// Functions that act on items yielded from [VHFIter].next().
///
/// Requires that the function is [Send] and [Sync].
pub type VHFItemFn = dyn Fn(<VHFIter as Iterator>::Item) -> WriteBlock + Send + Sync;

/// This fully contains all relevant mechanisms for taking the iterator output of
/// [super::super::VHFIter] for "in-flight processing."
#[derive(Clone)]
pub struct StreamFoldFunction {
    /// This the function that has to be applied to every chunked window from [super::super::VHF].next.
    pub func: Arc<VHFItemFn>,
    /// This is the number of windows to step by each time prior to par_iter.
    pub step_by: usize,
    /// This is the number of windows to pad to the start.
    pub pad: usize,
    /// This is the operation performed.
    pub op: StreamFoldOp,
}

impl StreamFoldFunction {
    /// Determine if the process of [self] creates any sort of decimation.
    /// Related: [super::super::VHF] has to determine the number of elements to read.
    pub(in super::super) fn effective_decimation_factor(&self) -> NonZeroUsize {
        match self.op {
            StreamFoldOp::None => unsafe { NonZeroUsize::new_unchecked(1) },
            StreamFoldOp::Map(None) => unsafe { NonZeroUsize::new_unchecked(1) },
            StreamFoldOp::Map(Some(MapArg {
                effective_decimation: e,
                ..
            })) => e,
            // StreamFoldOp::Reduce(_) => 1 //?
        }
    }

    /// Determine the number of elements dropped, starting from the first word from FPGA, up to,
    /// and not including the first element written to the file.
    pub fn words_dropped_before_first_write(&self) -> Result<i64> {
        match &self.op {
            StreamFoldOp::None => Ok(0),
            StreamFoldOp::Map(None) => Ok(0),
            StreamFoldOp::Map(Some(MapArg {
                num_before_first_drop: n,
                ..
            })) => {
                let n: usize = (*n).into();
                n.try_into().map_err(|_| {
                    log::error!("Could not get num_words_dropped as i64!");
                    Error::User
                })
            }
        }
    }
}

impl PartialEq for StreamFoldFunction {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.func), Arc::as_ptr(&other.func))
            && self.step_by == other.step_by
            && self.pad == other.pad
            && self.op == other.op
    }
}

impl Eq for StreamFoldFunction {}

impl std::fmt::Debug for StreamFoldFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op_str = match &self.op {
            StreamFoldOp::None => "none".to_string(),
            StreamFoldOp::Map(None) => "map: None".to_string(),
            StreamFoldOp::Map(Some(e)) => format!("map: Some({e:?})"),
        };

        f.debug_struct("StreamFold")
            .field("func", &"...")
            .field("step_by", &self.step_by)
            .field("pad", &self.pad)
            .field("op", &op_str)
            .finish()
    }
}

/// Determines the mode of operation on [super::super::VHF].next.
// Used in [StreamFoldFunction].
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
    Map(Option<MapArg>),
}

/// With the context of [super::super::Config::skip_num], [super::StreamFold] will lead to a decrease in number of
/// elements between the FPGA and what is written to the file.
/// This struct contains all arguments specific to [StreamFoldOp::Map] that would otherwise
/// definitely not make sense to be in [StreamFoldOp::None].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MapArg {
    /// This number summarizes the possibly multiple steps performed by [super::StreamFold::func].
    pub effective_decimation: NonZeroUsize,
    /// This is the number of elements that are "dropped" before the first element is written to
    /// file.
    pub num_before_first_drop: NonZeroUsize,
}
