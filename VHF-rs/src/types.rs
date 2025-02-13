/// This is one word of VHF data.
pub type RawVHFWord = u64;

/// Unpacking a [RawVHFWord].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IQMTriplet(pub i32, pub i32, pub i16); // (I, Q, M)

impl From<RawVHFWord> for IQMTriplet {
    #[inline(always)]
    fn from(value: RawVHFWord) -> Self {
        raw_to_triplet(value)
    }
}

impl From<&RawVHFWord> for IQMTriplet {
    #[inline(always)]
    fn from(value: &RawVHFWord) -> Self {
        // Deref &u64 -> u64 is not const.
        let i = (value >> 24) & 0xFFFFFF;
        let i = (i.wrapping_sub((i >> 23) * (1 << 24))) as i32;
        let q = value & 0xFFFFFF;
        let q = (q.wrapping_sub((q >> 23) * (1 << 24))) as i32;
        let m = (value >> 48) as i16;
        Self(i, q, m)
    }
}

#[inline(always)]
const fn raw_to_triplet(value: RawVHFWord) -> IQMTriplet {
    let i = (value >> 24) & 0xFFFFFF;
    let i = (i.wrapping_sub((i >> 23) * (1 << 24))) as i32;
    let q = value & 0xFFFFFF;
    let q = (q.wrapping_sub((q >> 23) * (1 << 24))) as i32;
    let m = (value >> 48) as i16;
    IQMTriplet(i, q, m)
}

impl From<IQMTriplet> for RawVHFWord {
    #[inline(always)]
    fn from(value: IQMTriplet) -> Self {
        triplet_to_raw(&value)
    }
}

impl From<&IQMTriplet> for RawVHFWord {
    #[inline(always)]
    fn from(value: &IQMTriplet) -> Self {
        triplet_to_raw(value)
    }
}

#[inline(always)]
const fn triplet_to_raw(value: &IQMTriplet) -> RawVHFWord {
    let i = ((-(1 << 24) + value.0) & 0xFFFFFF) as u64;
    let q = ((-(1 << 24) + value.1) & 0xFFFFFF) as u64;
    let m = (value.2 as u16) as u64;
    (m << 48) | (i << 24) | (q << 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn iqm_triplet_decode_encode() {
        let raw = 0x7FFF7FFFFF7FFFFF;
        let expected = IQMTriplet((1 << 23) - 1, (1 << 23) - 1, (1i16 << 15).wrapping_sub(1));
        assert_eq!(IQMTriplet::from(raw), expected);
        log::trace!("raw = {raw:x}");
        assert_eq!(RawVHFWord::from(expected), raw);

        let raw = 0x8000800000800000;
        let expected = IQMTriplet(-(1 << 23), -(1 << 23), 1i16 << 15);
        assert_eq!(IQMTriplet::from(raw), expected);
        assert_eq!(RawVHFWord::from(expected), raw);

        let raw = 0x7FFF800000800000;
        let expected = IQMTriplet(-(1 << 23), -(1 << 23), (1i16 << 15).wrapping_sub(1));
        assert_eq!(IQMTriplet::from(raw), expected);
        assert_eq!(RawVHFWord::from(expected), raw);

        for i in [-(1 << 23), -100, -1, 0, 1, 200, 0x7FFFFF] {
            for q in [-(1 << 23), -230, -1, 0, 1, 200, 0x7FFFFF] {
                for m in [0x8000u16 as i16, -1630, -1, 0, 1, 800, 0x7FFF] {
                    let triplet = IQMTriplet(i, q, m);
                    let result = IQMTriplet::from(RawVHFWord::from(&triplet));
                    assert_eq!(triplet, result);
                }
            }
        }
    }
}
