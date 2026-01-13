use super::consts::MMAP_PAGE_LEN;
use vhf_common::data_types::{Polar, RawVHFWord};

use std::f64::consts::TAU;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

/// Zero in [RawVHFWord].
pub struct ZeroArr {
    total_len: usize,
    current_idx: AtomicUsize,
    engine_running: Arc<AtomicBool>,
}

impl Iterator for ZeroArr {
    type Item = RawVHFWord;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx.load(Ordering::Acquire) >= self.total_len {
            self.engine_running.fetch_and(false, Ordering::AcqRel);
            None
        } else {
            self.current_idx.fetch_add(1, Ordering::Relaxed);
            Some(RawVHFWord::from(0))
        }
    }
}

impl ZeroArr {
    /// Creates [ZeroArr] which yields 0s for total_len number of elements.
    /// Strongly recommended to have an integer multiple of [MMAP_PAGE_LEN].
    pub fn new(total_len: usize, engine_running: Arc<AtomicBool>) -> Self {
        if !total_len.is_multiple_of(MMAP_PAGE_LEN) {
            log::warn!("ZeroArr did not receive an integer multiple of MMAP_PAGE_LEN");
        }
        Self {
            total_len,
            current_idx: AtomicUsize::new(0),
            engine_running,
        }
    }
}

/// Linear in [RawVHFWord].
pub struct LinearArr {
    total_len: u64,
    current_idx: AtomicU64,
    engine_running: Arc<AtomicBool>,
}

impl Iterator for LinearArr {
    type Item = RawVHFWord;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx.load(Ordering::Acquire) >= self.total_len {
            self.engine_running.fetch_and(false, Ordering::AcqRel);
            None
        } else {
            let prev = self.current_idx.fetch_add(1, Ordering::AcqRel);
            Some(RawVHFWord::from(prev))
        }
    }
}

impl LinearArr {
    pub fn new(total_len: usize, engine_running: Arc<AtomicBool>) -> Self {
        if !total_len.is_multiple_of(MMAP_PAGE_LEN) {
            log::warn!("LinearArr did not receive an integer multiple of MMAP_PAGE_LEN");
        }
        Self {
            total_len: total_len.try_into().unwrap(),
            current_idx: AtomicU64::new(0),
            engine_running,
        }
    }
}

/// Linear in phase.
pub struct LinearPhaseArr {
    total_len: u64,
    current_idx: AtomicU64,
    c: f64,
    m: f64,
    radius: f64,
    engine_running: Arc<AtomicBool>,
}

impl Iterator for LinearPhaseArr {
    type Item = RawVHFWord;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx.load(Ordering::Acquire) >= self.total_len {
            self.engine_running.fetch_and(false, Ordering::AcqRel);
            None
        } else {
            let j = self.current_idx.fetch_add(1, Ordering::AcqRel) as f64;
            let curr_phase = self.m * j + self.c;

            let polar = Polar {
                radius: self.radius,
                phase: curr_phase * TAU,
            };

            Some(polar.into())
        }
    }
}

impl LinearPhaseArr {
    /// Arguments:
    ///
    /// - total_len: Number of elements in iterator.
    /// - initial_params: (radius, initial_reduced_phase, reduced_gradient)
    ///   Radius is the value of the signal.
    ///   Reduced phase translates to Unwrapped phase / TAU.
    pub(super) fn new(
        total_len: usize,
        initial_params: (f64, f64, f64),
        engine_running: Arc<AtomicBool>,
    ) -> Self {
        if !total_len.is_multiple_of(MMAP_PAGE_LEN) {
            log::warn!("LinearArr did not receive an integer multiple of MMAP_PAGE_LEN");
        }
        Self {
            total_len: total_len.try_into().unwrap(),
            current_idx: AtomicU64::new(0),
            radius: initial_params.0,
            c: initial_params.1,
            m: initial_params.2,
            engine_running,
        }
    }
}

impl Clone for LinearPhaseArr {
    /// XXX: This will detach from the [`engine_running`].
    fn clone(&self) -> Self {
        Self {
            current_idx: AtomicU64::new(self.current_idx.load(Ordering::Acquire)),
            engine_running: Arc::new(AtomicBool::new(false)),
            ..*self
        }
    }
}

/// Sinusoidal in phase.
pub struct SineArr {
    total_len: usize,
    current_idx: AtomicUsize,
    engine_running: Arc<AtomicBool>,
    phase_ampl: f64,
    phase_angular_frequency: f64,
    phase_offset: f64,
    signal_ampl: f64,
    phase_y_offset: f64,
}

impl Iterator for SineArr {
    type Item = RawVHFWord;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx.load(Ordering::Acquire) >= self.total_len {
            self.engine_running.fetch_and(false, Ordering::AcqRel);
            None
        } else {
            let i = self.current_idx.fetch_add(1, Ordering::Acquire);
            let p = Polar {
                radius: self.signal_ampl,
                phase: (i as f64)
                    .mul_add(self.phase_angular_frequency, self.phase_offset)
                    .sin()
                    .mul_add(self.phase_ampl, self.phase_y_offset),
            };
            Some(p.into())
        }
    }
}

impl SineArr {
    /// Create a new iterator that generates a sine function.
    /// Parameters:
    /// * `total_len`: How many elements the iterator should yield.
    /// * `engine_running`: For this iterator to stop the VHF engine when the thread spawned by
    ///   this function ends.
    /// * `params`: (phase_ampl, phase_angular_frequency, phase_offset, signal_ampl, y-offset)
    pub(super) fn new(
        total_len: usize,
        engine_running: Arc<AtomicBool>,
        params: (f64, f64, f64, f64, f64),
    ) -> Self {
        if !total_len.is_multiple_of(MMAP_PAGE_LEN) {
            log::warn!("SineArr did not receive an integer multiple of MMAP_PAGE_LEN");
        }
        Self {
            total_len,
            current_idx: AtomicUsize::new(0),
            engine_running,
            phase_ampl: params.0,
            phase_angular_frequency: params.1,
            phase_offset: params.2,
            signal_ampl: params.3,
            phase_y_offset: params.4,
        }
    }
}

impl Clone for SineArr {
    /// XXX: This will detach from the [`engine_running`].
    fn clone(&self) -> Self {
        Self {
            current_idx: AtomicUsize::new(self.current_idx.load(Ordering::Acquire)),
            engine_running: Arc::new(AtomicBool::new(false)),
            ..*self
        }
    }
}
