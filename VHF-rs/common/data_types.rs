//! Types associated to data created by VHF board.
use std::f64::consts::TAU;

/// This is one word of VHF data.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct RawVHFWord(u64);

impl From<u64> for RawVHFWord {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Into<u64> for RawVHFWord {
    fn into(self) -> u64 {
        self.0
    }
}

impl RawVHFWord {
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// This gives the phase / 2pi between i8::MAX and i8::MIN.
    pub fn wrapped_phase(&self) -> f64 {
        let Polar { radius: _, phase } = self.into();
        phase
    }

    pub fn as_triplet(&self) -> IQMTriplet {
        self.into()
    }
}

impl std::ops::Deref for RawVHFWord {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Unpacking a [RawVHFWord].
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct IQMTriplet(pub i32, pub i32, pub i16); // (I, Q, M)

impl std::fmt::Debug for IQMTriplet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("IQM")
            .field(&self.0)
            .field(&self.1)
            .field(&self.2)
            .finish()
    }
}

impl From<RawVHFWord> for IQMTriplet {
    #[inline(always)]
    fn from(value: RawVHFWord) -> Self {
        raw_to_triplet(&value)
    }
}

impl From<&RawVHFWord> for IQMTriplet {
    #[inline(always)]
    fn from(value: &RawVHFWord) -> Self {
        raw_to_triplet(value)
    }
}

#[inline(always)]
const fn raw_to_triplet(value: &RawVHFWord) -> IQMTriplet {
    let value = value.0;
    let i = (value >> 24) & 0xFFFFFF;
    let i = (i.wrapping_sub((i >> 23) * (1 << 24))) as i32;
    let q = value & 0xFFFFFF;
    let q = (q.wrapping_sub((q >> 23) * (1 << 24))) as i32;
    let m = (value >> 48) as i16;
    IQMTriplet(i, q, m)
}

impl From<&Polar> for IQMTriplet {
    #[inline]
    fn from(value: &Polar) -> Self {
        let rem_phase = value.phase.rem_euclid(TAU); // [0, TAU)
        let (i, q) = (
            // Despite the name, I(n-phase) acts as y-coordinate across all definitions.
            (value.radius * rem_phase.sin()).round() as i32,
            // Despite the name Q(uadrature-phase) act as x-coordinate across all definitions.
            (value.radius * rem_phase.cos()).round() as i32,
        ); // Need to force to i32 before making new_phase otherwise m will be broken
        let new_phase = (i as f64).atan2(q as f64); // (-Pi, Pi]

        // rounding of (I, Q) can trip up the generation of m
        let diff = value.phase / TAU - new_phase / TAU;
        let m = diff.round();

        IQMTriplet(i, q, (m as i64 & 0xFFFF) as i16)
    }
}

impl From<Polar> for IQMTriplet {
    #[inline]
    fn from(value: Polar) -> Self {
        (&value).into()
    }
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
    RawVHFWord((m << 48) | (i << 24) | (q << 0))
}

impl From<&Polar> for RawVHFWord {
    #[inline]
    fn from(value: &Polar) -> Self {
        let iqm: IQMTriplet = value.into();
        (&iqm).into()
    }
}

impl From<Polar> for RawVHFWord {
    #[inline]
    fn from(value: Polar) -> Self {
        (&value).into()
    }
}

/// This is the polar representation of a data point. The phase here always denotes the wrapped phase.  
/// Wrapped here denotes being bound within i16::MIN to i16::MAX for m.
#[derive(Copy, Clone, Debug)]
pub struct Polar {
    pub radius: f64,
    pub phase: f64,
}

impl Polar {
    /// This gives the wrapped phase in (0, 2Pi].
    /// This is to align with atan2 for nonzero radius, which has a codomain of (-Pi, Pi].
    #[cfg(test)]
    #[inline]
    fn projected_phase(&self) -> f64 {
        let reduced_unwrapped = self.phase / TAU;
        //     fract    :add:         mod
        // (-X.f) -> -0.f -> 1. - 0.f -> 1. - 0.f
        // ( X.f) ->  0.f ->      1.f ->      0.f
        let fract = reduced_unwrapped.fract();
        let reduced_projected_phase = (fract + 1.) % 1.;
        let reduced_projected_phase_forced =
            (reduced_projected_phase == 0.0) as u8 as f64 + reduced_projected_phase;
        reduced_projected_phase_forced * TAU
    }

    /// Test for approximate equivalence while accounting for wrapping around pi.
    #[cfg(test)]
    fn approx_eq(&self, other: &Self) -> bool {
        let sp = self.projected_phase();
        let op = other.projected_phase();

        debug_assert!(0. < sp && sp <= TAU);
        debug_assert!(0. < op && op <= TAU);

        let radius = self.radius.min(other.radius);
        let e = 2. * (1. / radius).atan();

        use approx::relative_eq;
        relative_eq!(sp, op, epsilon = e)
            || relative_eq!(sp, op + TAU, epsilon = e)
            || relative_eq!(sp + TAU, op, epsilon = e)
    }
}

impl From<&RawVHFWord> for Polar {
    #[inline]
    fn from(value: &RawVHFWord) -> Self {
        let triple: IQMTriplet = value.into();
        (&triple).into()
    }
}

impl From<RawVHFWord> for Polar {
    #[inline]
    fn from(value: RawVHFWord) -> Self {
        (&value).into()
    }
}

impl From<&IQMTriplet> for Polar {
    fn from(value: &IQMTriplet) -> Self {
        let &IQMTriplet(i, q, m) = value;
        Polar {
            radius: (i as f64).hypot(q as f64),
            phase: (m as f64).mul_add(TAU, (i as f64).atan2(q as f64)),
        }
    }
}

impl From<IQMTriplet> for Polar {
    fn from(value: IQMTriplet) -> Self {
        (&value).into()
    }
}

/// Folding or Processing often will record where in the stream does a `m_overflow` event occurs, i.e.:
/// when the [IQMTriplet] has the `m` value have a over(under)flow occurrence.
#[derive(Debug, PartialEq, Eq)]
pub struct MOverflowRaw(pub usize, pub i8);

impl MOverflowRaw {
    /// Lowers the usize by offset amount, without being less than 0.
    /// # Unexpected behaviour
    /// If self.idx < offset, the function is meaningless, but returns 0.
    #[inline]
    pub fn offset_neg(self, offset: usize) -> Self {
        Self(self.0.saturating_sub(offset), self.1)
    }
}

impl From<(usize, i8)> for MOverflowRaw {
    #[inline(always)]
    fn from(value: (usize, i8)) -> Self {
        Self(value.0, value.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn iqm_triplet_decode_encode() {
        let raw: RawVHFWord = 0x7FFF7FFFFF7FFFFF.into();
        let expected = IQMTriplet((1 << 23) - 1, (1 << 23) - 1, (1i16 << 15).wrapping_sub(1));
        assert_eq!(IQMTriplet::from(raw), expected);
        log::trace!("raw = {:x}", &raw.0);
        assert_eq!(RawVHFWord::from(expected), raw);

        let raw: RawVHFWord = 0x8000800000800000.into();
        let expected = IQMTriplet(-(1 << 23), -(1 << 23), 1i16 << 15);
        assert_eq!(IQMTriplet::from(raw), expected);
        assert_eq!(RawVHFWord::from(expected), raw);

        let raw: RawVHFWord = 0x7FFF800000800000.into();
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

    #[test]
    fn triplet_to_from_polar() {
        let num_pts = 1000;
        let phase_multiple = TAU / 64.;

        for radius in [10., 200., 1000., i32::MAX as f64 / 2.] {
            for p_i in -num_pts..=num_pts {
                for m_offsets in [0f64, -1., 1.] {
                    let phase = p_i as f64 * phase_multiple;

                    let polar = Polar {
                        radius,
                        phase: phase + u16::MAX as f64 * m_offsets,
                    };
                    let triplet: IQMTriplet = (&polar).into();
                    let result: Polar = triplet.into();

                    assert!((result.radius - polar.radius).abs() <= 1.5);
                    assert!(result.approx_eq(&polar));
                }
            }
        }
    }

    #[test]
    fn polar_to_from_triplet() {
        log::info!("Testing known problematic cases");
        for i in [-7000, 0, 7000] {
            for q in [-7000, 0, 7000] {
                for m in [-10, -5, -4, 0, 4, 5, 10] {
                    if i == 0 && q == 0 {
                        continue;
                    };
                    let triplet = IQMTriplet(i, q, m);
                    let polar: Polar = triplet.into();
                    assert_eq!(triplet, polar.into());
                }
            }
        }

        use rand::Rng;
        use rand::distr::Uniform;

        let mut rng1 = rand::rng();
        let mut rng2 = rand::rng();
        let mut rng3 = rand::rng();
        let uni = Uniform::try_from(i32::MIN..i32::MAX).expect("Could not make uniform dist");
        let m_uni = Uniform::try_from(-(1 << 10)..(1 << 10)).expect("Could not make uniform dist");
        let num_points = 100;

        log::info!("Testing by random sample");
        let is = (&mut rng1).sample_iter(uni).take(num_points).into_iter();
        for i in is {
            let qs = (&mut rng2).sample_iter(uni).take(num_points).into_iter();
            for q in qs {
                let ms = (&mut rng3).sample_iter(m_uni).take(30).into_iter();
                for m in ms {
                    if i == 0 && q == 0 {
                        continue;
                    }
                    let triplet = IQMTriplet(i, q, m);
                    let polar: Polar = triplet.into();
                    assert_eq!(triplet, polar.into());
                }
            }
        }
    }
}
