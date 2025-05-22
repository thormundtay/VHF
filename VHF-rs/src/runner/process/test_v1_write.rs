//! This file is to test if [super::super::Writer] works.

use super::super::config::Configs;
use super::super::fold::StreamFold;
use super::super::writer::{V1Writer, VHFWriter};
use super::consts::MMAP_PAGE_LEN;
use super::test_vhf::{debug_vhf_new, push_arc_pages};
use super::test_vhf_step_fold::SineArr;
use super::*;

use jiff::Zoned;
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::ffi::CString;
use std::time::Duration;
use tempfile::TempDir;
use test_log::test;

/// Write a file that has data with m-overflow.
/// This test will fail if [super::test_vhf_step_fold::stepped_nonoverlapping_identity_b] fails.
#[test]
fn writes_correct_header() {
    let debug_vhf_total_len = 4 * VHF_MMAP_WINDOW_LEN;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let (debug_vhf, dbg_vhf_buffer, eng) =
        debug_vhf_new(NonZeroUsize::new(debug_vhf_total_len).unwrap());

    let total_elements = total_window_len * MMAP_PAGE_LEN;
    let StreamFold::None(params) = StreamFold::none_default() else {
        log::error!("Nonoverlapping windows are not StreamFold::None variant.");
        panic!()
    };

    // Define the signal we are testing for.
    let ampl = 5000f64;
    let ang_freq = TAU / (MMAP_PAGE_LEN as f64 / 2. + 1.);
    let phase_offset = 1.2f64;
    let signal_radius = 5000f64;

    let signal = SineArr::new(
        total_elements,
        eng.clone(),
        (
            ampl,
            ang_freq,
            phase_offset,
            signal_radius,
            TAU * 0x7FFF as f64,
        ),
    );
    let _signal_expected = signal.clone(); // This will lose the engine

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread = push_arc_pages(dbg_vhf_buffer, signal, Duration::new(0, 100), eng)
        .expect("push_arc_pages failed");

    let time_start = Zoned::now();
    let mut config = Configs::new(None).expect("Config struct could not be made");
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    config.save_dir = (*tmp_dir.path()).into();
    config.num_samples = 1 << 18;
    config.verbosity = 3;
    log::info!("save_dir = {:?}", &config.save_dir);

    let mut writer = V1Writer::new(&config, time_start);
    debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v1_writer failed");

    // TODO: Check for correctness of written data.

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}

#[test]
fn creates_multiple_files() {
    let scale_elements = 8;
    let file_save_size = 2;

    debug_assert!(
        super::super::writer::FILE_LAZY_LEN < VHF_MMAP_WINDOW_LEN * MMAP_PAGE_LEN * file_save_size
    );
    let debug_vhf_total_len = VHF_MMAP_WINDOW_LEN * scale_elements;
    log::info!("debug_vhf_total_len = {}", &debug_vhf_total_len);
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let (debug_vhf, dbg_vhf_buffer, eng) =
        debug_vhf_new(NonZeroUsize::new(debug_vhf_total_len).unwrap());

    let total_elements = total_window_len * MMAP_PAGE_LEN;
    let StreamFold::None(params) = StreamFold::none_default() else {
        log::error!("Nonoverlapping windows are not StreamFold::None variant.");
        panic!()
    };

    // Define the signal we are testing for.
    let ampl = 5000f64;
    let ang_freq = TAU / (MMAP_PAGE_LEN as f64 / 2. + 1.);
    let phase_offset = 1.2f64;
    let signal_radius = 5000f64;

    let signal = SineArr::new(
        total_elements,
        eng.clone(),
        (
            ampl,
            ang_freq,
            phase_offset,
            signal_radius,
            TAU * 0x7FFF as f64,
        ),
    );
    let _signal_expected = signal.clone(); // This will lose the engine

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread =
        push_arc_pages(dbg_vhf_buffer, signal, Duration::new(0, 10_000), eng)
            .expect("push_arc_pages failed");

    let time_start = Zoned::now();
    let mut config = Configs::new(None).expect("Config struct could not be made");
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    config.save_dir = (*tmp_dir.path()).into();
    config.num_samples = VHF_MMAP_WINDOW_LEN * MMAP_PAGE_LEN * 2;
    config.verbosity = 3;
    config.skip_num = i16::MAX as u16;
    let num_file = total_elements.div_ceil(config.num_samples);
    log::info!("save_dir = {:?}", &config.save_dir);
    config.num_files = num_file;
    let phasemeter_kwargs = HashMap::from([(
        CString::new("tmp_laser").unwrap(),
        CString::new("2").unwrap(),
    )]);

    config.phasemeter_kwargs = phasemeter_kwargs;

    let mut writer = V1Writer::new(&config, time_start);
    debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v1_writer failed");

    assert_eq!(
        std::fs::read_dir(config.save_dir).unwrap().count(),
        num_file
    );
    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}
