use std::f64::consts::{PI, TAU};

/// This is one word of VHF data.
pub type RawVHFWord = u64;

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
        raw_to_triplet(value)
    }
}

impl From<&RawVHFWord> for IQMTriplet {
    #[inline(always)]
    fn from(value: &RawVHFWord) -> Self {
        // Deref &u64 -> u64 is not const. Have to otherwise clone to use `raw_to_triplet`.
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

impl From<&Polar> for IQMTriplet {
    #[inline]
    fn from(value: &Polar) -> Self {
        let (m, rem_phase) = (
            ((value.phase / TAU).round() as i64 & 0xFFFF) as i16,
            value.phase % TAU,
        );
        let (i, q) = (
            // Despite the name, I(n-phase) acts as y-coordinate across all definitions.
            (value.radius * rem_phase.sin()).round() as i32,
            // Despite the name Q(uadrature-phase) act as x-coordinate across all definitions.
            (value.radius * rem_phase.cos()).round() as i32,
        );

        IQMTriplet(i, q, m)
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
    (m << 48) | (i << 24) | (q << 0)
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
    /// This gives the wrapped phase in [-Pi, Pi).
    #[inline]
    fn projected_phase(&self) -> f64 {
        let unwrapped = self.phase;
        if unwrapped >= PI {
            let t = (unwrapped - PI) / TAU;
            let t = t.floor() + 1.;
            -(t.mul_add(TAU, -unwrapped))
        } else if unwrapped < -PI {
            let u = (unwrapped + PI) / TAU;
            let u = u.ceil();
            u.mul_add(TAU, unwrapped)
        } else {
            unwrapped
        }
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

    #[test]
    fn triplet_to_from_polar() {
        use approx::assert_relative_eq;
        for radius in [10., 200., 1000., i32::MAX as f64 / 2.] {
            for phase in -20..=20 {
                let phase = phase as f64;

                let polar = Polar { radius, phase };
                let triplet: IQMTriplet = (&polar).into();
                let result: Polar = triplet.into();

                assert!(triplet.2.abs_diff((phase / TAU) as i16) <= 1);
                assert!((result.radius - polar.radius).abs() <= 1.5);
                assert_relative_eq!(
                    result.projected_phase(),
                    polar.projected_phase(),
                    epsilon = 2. * (1. / radius).atan()
                );
            }
        }
    }

    #[test]
    fn polar_to_from_triplet() {
        for i in (0..(1 << 13)).into_iter().step_by(2000) {
            for q in (0..(1 << 13)).into_iter().step_by(3000) {
                for m in (0..(1 << 10)).into_iter().step_by(100) {
                    let triplet = IQMTriplet(i, q, m);
                    let polar: Polar = triplet.into();
                    assert_eq!(triplet, polar.into());
                }
            }
        }
    }
}
