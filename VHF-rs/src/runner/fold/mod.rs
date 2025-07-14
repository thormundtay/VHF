pub mod repr;

use super::process::{
    consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN},
    pages::MmapPage,
};
use super::writer::WriteBlock;
use crate::{Error, Result, parser::consts::M_OVERFLOW};
use repr::Representation;
use serde::Serialize;
use std::{cmp::Ordering, hint::unreachable_unchecked, num::NonZeroUsize, ops::Deref, sync::Arc};
use vhf_common::data_types::{IQMTriplet, RawVHFWord};

/// Bounding [MOverflowWrite] limits.
const M_OVERFLOW_IDX_MAX: usize = usize::MAX >> 1;
/// [super::fold] often will record where in the stream does a `m_overflow` event occurs, i.e.:
/// when the [IQMTriplet] has the `m` value have a over(under)flow occurrence.
/// See TryFrom implementation.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct MOverflowRaw(pub usize, pub i8);
/// Compacted representation of [MOverflowRaw] into 8 bytes for file-writing reasons.  
/// See: [super::writer::V2BinWriter].
pub(super) type MOverflowWrite = i64;

/// This fully describes and contains all relevant mechanisms for taking the iterator output of
/// [super::VHFIter] for "in-flight processing."
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
            && self.op == other.op
            && self.repr == other.repr
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
    Map(Option<MapArg>),
}

/// With the context of [super::Config::skip_num], [StreamFold] will lead to a decrease in number of
/// elements between the FPGA and what is written to the file.
/// This struct contains all arguments specific to [StreamFoldOp::Map] that would otherwise
/// definitely not make sense to be in [StreamFoldOp::None].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MapArg {
    /// This number summarizes the possibly multiple steps performed by [StreamFold::func].
    pub effective_decimation: NonZeroUsize,
    /// This is the number of elements that are "dropped" before the first element is written to
    /// file.
    pub num_before_first_drop: NonZeroUsize,
}

impl std::fmt::Debug for StreamFold {
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

    /// Determine if the process of [self] creates any sort of decimation.
    /// Related: [super::VHF] has to determine the number of elements to read.
    pub(super) fn effective_decimation_factor(&self) -> NonZeroUsize {
        match self.op {
            StreamFoldOp::None => unsafe { NonZeroUsize::new(1).unwrap_unchecked() },
            StreamFoldOp::Map(None) => unsafe { NonZeroUsize::new(1).unwrap_unchecked() },
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

impl TryFrom<&MOverflowRaw> for MOverflowWrite {
    type Error = Error;

    /// Converts [MOverflowRaw] into a standardized representation of 64-bits. The most significant
    /// bit (MSB) denotes the sign change, where 0 denotes +1 and 1 denotes -1. After zeroing the MSB, interpreting as index.
    ///
    /// # Errors
    /// When the index of [MOverflowRaw.0] is too large.
    fn try_from(value: &MOverflowRaw) -> Result<MOverflowWrite> {
        if value.0 > M_OVERFLOW_IDX_MAX {
            return Err(Error::ExcessData);
        }
        match value.1 {
            1 => {
                let idx: u64 = value.0.try_into().unwrap(); // Safety: M_OVERFLOW_IDX_MAX
                let idx = idx as i64;
                debug_assert!(idx >> 63 == 0);
                Ok(idx)
            }
            -1 => {
                let idx: u64 = value.0.try_into().unwrap(); // Safety: M_OVERFLOW_IDX_MAX
                let idx = idx as i64 + (1 << 63);
                debug_assert!(idx >> 63 == 1);
                Ok(idx)
            }
            #[cfg(test)]
            _ => panic!("Unrecognised overflow-raw sign"),
            #[cfg(not(test))]
            _ => unsafe { unreachable_unchecked() },
        }
    }
}

impl TryFrom<MOverflowRaw> for MOverflowWrite {
    type Error = Error;

    /// Converts [MOverflowRaw] into a standardized representation of 64-bits. The most significant
    /// bit (MSB) denotes the sign change, where 0 denotes +1 and 1 denotes -1. After zeroing the MSB, interpreting as index.
    ///
    /// # Errors
    /// When the index of [MOverflowRaw.0] is too large.
    #[inline(always)]
    fn try_from(value: MOverflowRaw) -> Result<MOverflowWrite> {
        (&value).try_into()
    }
}

impl TryFrom<&MOverflowWrite> for MOverflowRaw {
    type Error = Error;

    fn try_from(value: &MOverflowWrite) -> Result<MOverflowRaw> {
        let sign = *value >> 63;
        let sign = if sign == 0 {
            Ok(1)
        } else if sign == 1 {
            Ok(-1)
        } else {
            return Err(Error::InternalInconsistency);
        }?;

        Ok(MOverflowRaw(
            (*value & ((u64::MAX >> 1) as i64))
                .try_into()
                .map_err(|_| Error::ExcessData)?,
            sign as i8,
        ))
    }
}

impl From<(usize, i8)> for MOverflowRaw {
    #[inline(always)]
    fn from(value: (usize, i8)) -> Self {
        Self(value.0, value.1)
    }
}

impl MOverflowRaw {
    /// Lowers the usize by offset amount, without being less than 0.
    /// # Unexpected behaviour
    /// If self.idx < offset, the function is meaningless, but returns 0.
    #[inline]
    pub(super) fn offset_neg(self, offset: usize) -> Self {
        Self(self.0.saturating_sub(offset), self.1)
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

    #[test]
    fn packing_m_overflow() {
        let x = MOverflowRaw(2, 1);
        let y: MOverflowWrite = (&x).try_into().unwrap();
        let z: MOverflowRaw = (&y).try_into().unwrap();

        assert_eq!(x, z);
    }
}
