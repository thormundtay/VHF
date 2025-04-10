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

use std::f64::consts::TAU;
use test_log::test;

/// Check that without any m-overflow, StreamFold behaves to expectation.
#[test]
fn stepped_nonoverlapping_identity_a() {
    let debug_vhf_total_len = 4 * VHF_MMAP_WINDOW_LEN;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let debug_vhf = debug_vhf_new(NonZeroU64::new(debug_vhf_total_len as u64).unwrap());

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
            .for_each(|x| buf.push_back(x));
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    };

    let result: Vec<RawVHFWord> = debug_vhf
        .into_iter()
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
    let debug_vhf = debug_vhf_new(NonZeroU64::new(debug_vhf_total_len as u64).unwrap());

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
            .for_each(|x| buf.push_back(x));
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    };

    let result: Vec<RawVHFWord> = debug_vhf
        .into_iter()
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
