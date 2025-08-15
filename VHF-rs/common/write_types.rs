//! Types associated specifically to writing (and reading) to files.

use crate::data_types::MOverflowRaw;
use crate::{Error, Result};
use std::ops::Deref;

/// Bounding [MOverflowWrite] limits.
const M_OVERFLOW_IDX_MAX: usize = usize::MAX >> 1;

/// Compacted representation of [MOverflowRaw] into 8 bytes for file-writing reasons.  
/// See: [VHF::runner::writer::V2BinWriter].
#[repr(transparent)]
pub struct MOverflowWrite(pub i64);

impl Deref for MOverflowWrite {
    type Target = i64;
    fn deref(&self) -> &Self::Target {
        &self.0
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
                debug_assert_eq!((idx as u64) >> 63, 0);
                Ok(MOverflowWrite(idx))
            }
            -1 => {
                let idx: u64 = value.0.try_into().unwrap(); // Safety: M_OVERFLOW_IDX_MAX
                let idx = idx as i64 + (1 << 63);
                debug_assert_eq!((idx as u64) >> 63, 1);
                Ok(MOverflowWrite(idx))
            }
            #[cfg(test)]
            _ => panic!("Unrecognised overflow-raw sign"),
            #[cfg(not(test))]
            _ => unsafe {
                use std::hint::unreachable_unchecked;
                unreachable_unchecked()
            },
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
        let value = value.0;
        let sign = (value as u64) >> 63;
        let sign = if sign == 0 {
            Ok(1)
        } else if sign == 1 {
            Ok(-1)
        } else {
            Err(Error::InternalInconsistency)
        }?;

        Ok(MOverflowRaw(
            (value & ((u64::MAX >> 1) as i64))
                .try_into()
                .map_err(|_| Error::ExcessData)?,
            sign as i8,
        ))
    }
}

impl TryFrom<MOverflowWrite> for MOverflowRaw {
    type Error = Error;

    #[inline(always)]
    fn try_from(value: MOverflowWrite) -> Result<MOverflowRaw> {
        (&value).try_into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packing_m_overflow() {
        let x = MOverflowRaw(2, 1);
        let y: MOverflowWrite = (&x).try_into().unwrap();
        let z: MOverflowRaw = (&y).try_into().unwrap();

        assert_eq!(x, z);
    }
}
