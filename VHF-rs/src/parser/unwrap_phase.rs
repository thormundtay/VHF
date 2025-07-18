//! Trait for taking any Parsing method and yielding the unwrapped phase.

use vhf_common::data_types::Polar;

use crate::{M, ReducedPhase, VHFWord, consts::M_OVERFLOW};
use std::{cmp::Ordering, f64::consts::TAU, hint::unreachable_unchecked};

/// Trait for converting a stream of [VHFWord] to Unwrapped phase.
pub trait VHFWordToUnwrappedPhase {
    type Output;

    /// Takes a Iterator of [VHFWord] and get an unwrapped [ReducedPhase]. This handles the 2 byte
    /// overflow.
    ///
    /// Arguments:
    /// - m_overflow
    ///   This corresponds to the offset associated to the first element. For example,
    ///   0 means that the 0th element would have it's m_value between i16::MIN to i16::MAX.
    ///   +1 means that the 0th element would have it's m_value between (u16::MAX*(+1) + i16::MIN)
    ///   to (u16::MAX*(+1) + i16::MAX).
    fn to_unwrapped_phase(self, m_overflow: M) -> Self::Output;
}

/// Trait for converting a stream of [VHFWord] to Unwrapped phase, releasing as an Iterator.
pub trait VHFWordToUnwrappedPhaseByIter {
    type Output;

    /// Takes a Iterator of [VHFWord] and get an unwrapped [ReducedPhase]. This handles the 2 byte
    /// overflow.
    ///
    /// Arguments:
    /// - m_overflow
    ///   This corresponds to the offset associated use std::thread::JoinHandle;to the first element. For example,
    ///   0 means that the 0th element would have it's m_value between i16::MIN to i16::MAX.
    ///   +1 means that the 0th element would have it's m_value between (u16::MAX*(+1) + i16::MIN)
    ///   to (u16::MAX*(+1) + i16::MAX).
    fn to_unwrapped_phase(self, m_overflow: M) -> impl Iterator<Item = Self::Output>;
}

/// Provisions the most unoptimized version, where only the phase is being streamed in.
impl<T> VHFWordToUnwrappedPhaseByIter for &mut T
where
    T: Iterator<Item = VHFWord>,
{
    type Output = ReducedPhase;

    /// # Panics
    /// Does not work on empty iterators
    fn to_unwrapped_phase(self, initial_m: M) -> impl Iterator<Item = Self::Output> {
        let first_word = self.next();

        if first_word.is_none() {
            log::error!("panics on empty");
            panic!("Received empty iterator");
        }

        // Avoid the need for peeking with use of .peekable().
        let iter = vec![first_word.clone().unwrap()].into_iter().chain(self);
        let mut curr_offset = initial_m;
        let mut prev_wrapped_triplet = first_word.clone().unwrap().as_triplet();
        let result = iter.scan(
            first_word.unwrap().wrapped_phase(),
            move |_, x: VHFWord| -> Option<Self::Output> {
                let curr_wrapped_triplet = x.as_triplet();

                let prev_m = prev_wrapped_triplet.2;
                let curr_m = curr_wrapped_triplet.2;
                if prev_m.abs_diff(curr_m) >= M_OVERFLOW {
                    match curr_m.cmp(&prev_m) {
                        Ordering::Less => {
                            curr_offset -= 1;
                        }
                        Ordering::Greater => {
                            curr_offset += 1;
                        }
                        // Safety: M_OVERFLOW check above.
                        Ordering::Equal => unsafe { unreachable_unchecked() },
                    }
                };

                let Polar {
                    radius: _,
                    phase: curr_wrapped_phase,
                } = &curr_wrapped_triplet.into();

                prev_wrapped_triplet = curr_wrapped_triplet;

                Some(
                    (curr_offset as f64).mul_add(-(u16::MAX as f64 + 1.), curr_wrapped_phase / TAU),
                )
            },
        );

        result.into_iter()
    }
}

#[cfg(test)]
mod unwrap {
    use crate::VHFWord;
    use test_log::test;

    #[test]
    #[should_panic]
    fn trivial_case_panics() {
        use super::VHFWordToUnwrappedPhaseByIter;
        let phases: Vec<VHFWord> = vec![];
        let iter = &mut phases.into_iter();

        let mut unwrapped_phases = iter.to_unwrapped_phase(1 as _);
        assert!(unwrapped_phases.next().is_none());
    }
}
