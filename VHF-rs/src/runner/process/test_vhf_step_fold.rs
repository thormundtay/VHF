//! This set of tests is more so to test that
//! ```
//! VHF.iter().step_by(x).map()
//! ```
//! behaves to expectation.

use super::super::fold::StreamFold;
use super::consts::MMAP_PAGE_LEN;
use super::test_vhf::{create_arc_pages, debug_vhf_new};
use super::*;
use crate::types::{IQMTriplet, Polar, RawVHFWord};

use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicUsize, Ordering};
use test_log::test;

pub(super) struct SineArr {
    total_len: AtomicUsize,
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
        if self.current_idx.load(Ordering::Acquire) >= self.total_len.load(Ordering::Acquire) {
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
        if total_len % MMAP_PAGE_LEN != 0 {
            log::warn!("ZeroArr did not receive an integer multiple of MMAP_PAGE_LEN");
        }
        Self {
            total_len: AtomicUsize::new(total_len),
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
    fn clone(&self) -> Self {
        Self {
            total_len: AtomicUsize::new(self.total_len.load(Ordering::Acquire)),
            current_idx: AtomicUsize::new(self.current_idx.load(Ordering::Acquire)),
            engine_running: Arc::new(AtomicBool::new(false)),
            ..*self
        }
    }
}

/// Check that without any m-overflow, StreamFold behaves to expectation.
#[test]
fn stepped_nonoverlapping_identity_a() {
    let debug_vhf_total_len = 4 * VHF_MMAP_WINDOW_LEN;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let debug_vhf = debug_vhf_new(NonZeroUsize::new(debug_vhf_total_len).unwrap());

    let total_elements = total_window_len * MMAP_PAGE_LEN;
    let StreamFold::None(params) = StreamFold::none_default() else {
        log::error!("Nonoverlapping windows are not StreamFold::None variant.");
        panic!()
    };

    // Define the signal we are testing for.
    let ampl = 5000f64;
    let ang_freq = TAU / (MMAP_PAGE_LEN as f64 / 2. + 1.);
    let phase_offset = 1.2f64;
    let signal_phase =
        (0..total_elements).map(|i| ampl * (i as f64).mul_add(ang_freq, phase_offset).sin());
    let signal_radius = 5000f64;
    let signal = signal_phase.map(|x| Polar {
        radius: signal_radius,
        phase: x,
    });

    // Add signal into pages.
    // We now add data into the buffer.
    if let Ok(mut buf) = debug_vhf.buffer.lock() {
        let tmp_signal: Vec<_> = signal.clone().map(|x| x.into()).collect();
        log::info!(
            "Number of pages placed into buffer = {}",
            create_arc_pages(&tmp_signal).len()
        );
        create_arc_pages(&tmp_signal)
            .into_iter()
            .for_each(|x| buf.push_back(x).expect("Push back failed."));
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    };

    let result: Vec<RawVHFWord> = debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .flat_map(|x| x.data.into_iter())
        .collect();

    let expected: Vec<RawVHFWord> = signal.map(|x| x.into()).collect();

    assert_eq!(result.len(), expected.len());
    result.into_iter().zip(expected).for_each(|(r, e)| {
        let IQMTriplet(ri, rq, rm) = r.into();
        let IQMTriplet(ei, eq, em) = e.into();
        assert_eq!(ri, ei);
        assert_eq!(rq, eq);
        assert_eq!(rm, em);
    });
}

/// Check that with any m-overflow, StreamFold behaves to expectation.
#[test]
fn stepped_nonoverlapping_identity_b() {
    let debug_vhf_total_len = 4 * VHF_MMAP_WINDOW_LEN;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let debug_vhf = debug_vhf_new(NonZeroUsize::new(debug_vhf_total_len).unwrap());

    let total_elements = total_window_len * MMAP_PAGE_LEN;
    let StreamFold::None(params) = StreamFold::none_default() else {
        log::error!("Nonoverlapping windows are not StreamFold::None variant.");
        panic!()
    };

    // Define the signal we are testing for.
    let ampl = 5000f64;
    let ang_freq = TAU / (MMAP_PAGE_LEN as f64 / 2. + 1.);
    let phase_offset = 1.2f64;
    let signal_phase =
        (0..total_elements).map(|i| ampl * (i as f64).mul_add(ang_freq, phase_offset).sin());
    let signal_radius = 5000f64;
    let signal = signal_phase.map(|x| Polar {
        radius: signal_radius,
        phase: x + (TAU * 0x7FFF as f64),
    });

    // Add signal into pages.
    // We now add data into the buffer.
    if let Ok(mut buf) = debug_vhf.buffer.lock() {
        let tmp_signal: Vec<_> = signal.clone().map(|x| x.into()).collect();
        log::info!(
            "Number of pages placed into buffer = {}",
            create_arc_pages(&tmp_signal).len()
        );
        create_arc_pages(&tmp_signal)
            .into_iter()
            .for_each(|x| buf.push_back(x).expect("Push back failed."));
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    };

    let result: Vec<RawVHFWord> = debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .flat_map(|x| x.data.into_iter())
        .collect();

    let expected: Vec<RawVHFWord> = signal.map(|x| x.into()).collect();

    assert_eq!(result.len(), expected.len());
    result.into_iter().zip(expected).for_each(|(r, e)| {
        let IQMTriplet(ri, rq, rm) = r.into();
        let IQMTriplet(ei, eq, em) = e.into();
        assert_eq!(ri, ei);
        assert_eq!(rq, eq);
        assert_eq!(rm, em);
    });
}

/// Check that without any m-overflow, StreamFold behaves to expectation.
#[test]
fn stepped_overlapping_identity_a() {
    let StreamFold::Map(params) = StreamFold::identity_default() else {
        log::error!("Nonoverlapping windows are not StreamFold::Map variant.");
        panic!()
    };

    let debug_vhf_total_len = 4 * params.step_by;
    let total_window_len = debug_vhf_total_len + params.step_by;
    let debug_vhf = debug_vhf_new(NonZeroUsize::new(debug_vhf_total_len).unwrap());
    let total_elements = total_window_len * MMAP_PAGE_LEN;

    // Define the signal we are testing for.
    let ampl = 5000f64;
    let ang_freq = TAU / (MMAP_PAGE_LEN as f64 / 2. + 1.);
    let phase_offset = 1.2f64;
    let signal_phase =
        (0..total_elements).map(|i| ampl * (i as f64).mul_add(ang_freq, phase_offset).sin());
    let signal_radius = 5000f64;
    let signal = signal_phase.map(|x| Polar {
        radius: signal_radius,
        phase: x,
    });

    // Add signal into pages.
    // We now add data into the buffer.
    if let Ok(mut buf) = debug_vhf.buffer.lock() {
        buf.push_back(MmapPage::Empty).expect("Push back failed.");
        let tmp_signal: Vec<_> = signal.clone().map(|x| x.into()).collect();
        log::info!(
            "Number of pages placed into buffer = {}",
            create_arc_pages(&tmp_signal).len()
        );
        create_arc_pages(&tmp_signal)
            .into_iter()
            .for_each(|x| buf.push_back(x).expect("Push back failed."));
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    };

    // We also need to test for sign overflow.
    let results: Vec<_> = debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .collect();
    let result_phase: Vec<RawVHFWord> = results
        .iter()
        .cloned()
        .flat_map(|x| x.data.into_iter())
        .collect();
    let result_overflow_idx: Vec<_> = results.into_iter().flat_map(|x| x.overflow()).collect();

    let expected_phase: Vec<RawVHFWord> = signal.map(|x| x.into()).collect();
    let expected_overflow_idx: Vec<(usize, i8)> = Vec::new();

    assert_eq!(result_phase.len(), expected_phase.len());
    result_phase
        .into_iter()
        .zip(expected_phase)
        .for_each(|(r, e)| {
            let IQMTriplet(ri, rq, rm) = r.into();
            let IQMTriplet(ei, eq, em) = e.into();
            assert_eq!(ri, ei);
            assert_eq!(rq, eq);
            assert_eq!(rm, em);
        });

    assert_eq!(result_overflow_idx.len(), expected_overflow_idx.len());
}

/// Check that with m-overflow, StreamFold behaves to expectation.
#[test]
fn stepped_overlapping_identity_b() {
    let StreamFold::Map(params) = StreamFold::identity_default() else {
        log::error!("Nonoverlapping windows are not StreamFold::Map variant.");
        panic!()
    };

    let total_window_len = params.step_by;
    let debug_vhf = debug_vhf_new(NonZeroUsize::new(total_window_len).unwrap());
    let total_elements = total_window_len * MMAP_PAGE_LEN;
    log::info!("total_elements = {}", total_elements);

    // Define the signal we are testing for.
    let ampl = 5000f64;
    let ang_freq = TAU / (MMAP_PAGE_LEN as f64 / 2. + 1.);
    let phase_offset = 1.2f64;
    let signal_phase =
        (0..total_elements).map(|i| ampl * (i as f64).mul_add(ang_freq, phase_offset).sin());
    let signal_radius = 5000f64;
    let signal = signal_phase.map(|x| Polar {
        radius: signal_radius,
        phase: x + (TAU * 0x7FFF as f64),
    });

    // Add signal into pages.
    // We now add data into the buffer.
    if let Ok(mut buf) = debug_vhf.buffer.lock() {
        buf.push_back(MmapPage::Empty).expect("Push back failed.");
        let tmp_signal: Vec<_> = signal.clone().map(|x| x.into()).collect();
        log::info!(
            "Number of pages placed into buffer = {}",
            create_arc_pages(&tmp_signal).len()
        );
        create_arc_pages(&tmp_signal)
            .into_iter()
            .for_each(|x| buf.push_back(x).expect("Push back failed."));
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    };

    // We also need to test for sign overflow.
    let results: Vec<_> = debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .collect();
    let result_phase: Vec<RawVHFWord> = results
        .iter()
        .cloned()
        .flat_map(|x| x.data.into_iter())
        .collect();
    let result_overflow_idx: Vec<_> = results.into_iter().flat_map(|x| x.overflow()).collect();

    let expected_phase: Vec<RawVHFWord> = signal.map(|x| x.into()).collect();
    let expected_overflow_idx: Vec<(usize, i8)> = {
        // + signs
        let plus = (1..)
            .map(|i| (TAU * i as f64 - phase_offset) / ang_freq)
            .take_while(|&i| i < total_elements as f64)
            .map(|idx| (idx.ceil() as usize, 1));
        // - signs
        let minus = (0..)
            .map(|i| (TAU * i as f64 + PI - phase_offset) / ang_freq)
            .take_while(|&i| i < total_elements as f64)
            .map(|idx| (idx.ceil() as usize, -1));
        let mut result: Vec<(usize, i8)> = plus.chain(minus).collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    };

    {
        let (i, sign) = result_overflow_idx.first().unwrap();
        let show: Vec<IQMTriplet> = (i - 1..=i + 1).map(|i| result_phase[i].into()).collect();
        log::info!(
            "Elements around the first sign change ({}) are: {:?}",
            sign,
            show
        );
        let (i, sign) = result_overflow_idx.iter().skip(1).next().unwrap();
        let show: Vec<IQMTriplet> = (i - 1..=i + 1).map(|i| result_phase[i].into()).collect();
        log::info!(
            "Elements around the second sign change ({}) are: {:?}",
            sign,
            show
        );
    }

    assert_eq!(result_phase.len(), expected_phase.len());
    result_phase
        .into_iter()
        .zip(expected_phase)
        .for_each(|(r, e)| {
            let IQMTriplet(ri, rq, rm) = r.into();
            let IQMTriplet(ei, eq, em) = e.into();
            assert_eq!(ri, ei);
            assert_eq!(rq, eq);
            assert_eq!(rm, em);
        });

    assert_eq!(result_overflow_idx, expected_overflow_idx);
}
