//! This file is to test if [super::super::Writer] works.

use super::super::config::Configs;
use super::super::fold::{StreamFold, StreamFoldOp};
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
    let debug_vhf_conf = Configs::default();
    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let total_elements = total_window_len * MMAP_PAGE_LEN;
    let params = StreamFold::none_default();
    matches!(params.op, StreamFoldOp::None);

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
    let push_arc_pages_thread = push_arc_pages(dbg_vhf_sender, signal, Duration::new(0, 100), eng)
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
    let scale_elements = 8; // scale number of generated elements
    let file_save_size = 2; // scale number of elements in file

    let debug_vhf_total_len = VHF_MMAP_WINDOW_LEN * scale_elements;
    log::info!("debug_vhf_total_len = {}", &debug_vhf_total_len);
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let debug_vhf_conf = Config::default();
    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let total_elements = total_window_len * MMAP_PAGE_LEN;
    let params = StreamFold::none_default();
    matches!(params.op, StreamFoldOp::None);

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
        push_arc_pages(dbg_vhf_sender, signal, Duration::new(0, 10_000), eng)
            .expect("push_arc_pages failed");

    let time_start = Zoned::now();
    let mut config = Configs::new(None).expect("Config struct could not be made");
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    config.save_dir = (*tmp_dir.path()).into();
    config.num_samples = VHF_MMAP_WINDOW_LEN * MMAP_PAGE_LEN * file_save_size;
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

/// There was a deadlock in creating a new file on the 7th gen intel machine
#[ignore = "large files"]
#[test]
fn creates_correct_multithreaded_files() {
    let scale_elements = 2usize.pow(15); // scale number of generated elements
    let file_save_size = 2usize.pow(14); // scale number of elements in file

    debug_assert!(
        super::super::writer::FILE_LAZY_LEN < VHF_MMAP_WINDOW_LEN * MMAP_PAGE_LEN * file_save_size
    );
    let debug_vhf_total_len = VHF_MMAP_WINDOW_LEN * scale_elements;
    log::info!("debug_vhf_total_len = {}", &debug_vhf_total_len);
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let debug_vhf_conf = Config::default();
    let (mut debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let total_elements = total_window_len * MMAP_PAGE_LEN;
    let params = StreamFold::none_default();
    matches!(params.op, StreamFoldOp::None);

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

    let thread_sleep = Duration::new(0, 10_000);
    // We want the vhf thread to keep up with arc_pages_thread
    debug_vhf.time_between_pages = thread_sleep.try_into().unwrap();

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread =
        push_arc_pages(dbg_vhf_sender, signal, thread_sleep, eng).expect("push_arc_pages failed");

    let time_start = Zoned::now();
    let mut config = Configs::new(None).expect("Config struct could not be made");
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    config.save_dir = (*tmp_dir.path()).into();
    config.num_samples = VHF_MMAP_WINDOW_LEN * MMAP_PAGE_LEN * file_save_size;
    config.verbosity = 3;
    config.skip_num = i16::MAX as u16;
    let num_file = total_elements.div_ceil(config.num_samples);
    log::info!("save_dir = {:?}", &config.save_dir);
    log::info!("num_files = {:?}", &num_file);
    log::info!(
        "num_samples = {:?}, this translates to {:?} MiB",
        &config.num_samples,
        &config.num_samples * 8 / 2usize.pow(20)
    );
    debug_assert!(
        num_file >= 2,
        "Point of this test is ensuring file writer does not lock up"
    );
    config.num_files = num_file;
    let phasemeter_kwargs = HashMap::from([(
        CString::new("tmp_laser").unwrap(),
        CString::new("2").unwrap(),
    )]);

    config.phasemeter_kwargs = phasemeter_kwargs;

    // Pull out from buffer and write to file in multithreaded
    let mut writer = V1Writer::new(&config, time_start);
    let vhf_iter = debug_vhf.iter();
    use pariter::IteratorExt;
    let body = pariter::scope(|scope| {
        vhf_iter
            .step_by(params.step_by)
            .parallel_map_scoped(scope, |x| (*params.func)(x))
            .try_for_each(|write_block| writer.write_data(write_block))
            .expect("Failed to write data");
    });

    match body {
        Ok(_) => log::info!("Run completed"),
        Err(e) => log::error!("Main loop occurred with error = {:?}", e),
    };

    // Check that this is a correct number of files.
    assert_eq!(
        std::fs::read_dir(config.save_dir).unwrap().count(),
        num_file
    );

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}
