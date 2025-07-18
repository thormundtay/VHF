//! This file is to test if [super::super::Writer] works.

use super::super::config::Configs;
use super::super::fold::{StreamFold, StreamFoldOp};
use super::super::writer::builder::Writers;
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

#[cfg(feature = "o3")]
use approx::assert_relative_eq;
#[cfg(feature = "o3")]
use jiff::ZonedRound;
#[cfg(feature = "o3")]
use std::num::NonZeroU64;
#[cfg(feature = "o3")]
use std::path::{Path, PathBuf};
#[cfg(feature = "o3")]
use vhf_parse::{VHFparse, v1::VHFparser as V1parser};

/// For a temp dir, check that there has only been a single file in it. Thereafter, yield the path
/// to it.
#[cfg(feature = "o3")]
fn get_only_file(tmpdir: &Path) -> Result<PathBuf> {
    if !tmpdir.is_dir() {
        return Err(Error::InternalInconsistency);
    }
    let files: Vec<_> = tmpdir
        .read_dir()
        .expect("Dir could not be read")
        .filter_map(|d| d.ok())
        .collect();
    if files.len() != 1 {
        return Err(Error::InternalInconsistency);
    }

    files
        .into_iter()
        .next()
        .map(|d| d.path())
        .ok_or(Error::InternalInconsistency)
}

/// Write a file that has data with m-overflow.
/// This test will fail if [super::test_vhf_step_fold::stepped_nonoverlapping_identity_b] fails.
#[test]
fn writes_correct_file() {
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
    let mut signal_expected = signal.clone(); // This will lose the engine

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread = push_arc_pages(dbg_vhf_sender, signal, Duration::new(0, 100), eng)
        .expect("push_arc_pages failed");

    let time_start = Zoned::now();
    let mut config = Configs::new(None).expect("Config struct could not be made");
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    config.save_to_file = true;
    config.save_dir = (*tmp_dir.path()).into();
    config.num_samples = total_elements;
    config.verbosity = 3;
    log::info!("save_dir = {:?}", &config.save_dir);

    let builder = config.file_writer().unwrap();
    matches!(builder.writer_type, Writers::V1(_));
    let mut writer = builder.with_start_time(time_start.clone()).build();

    debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v1_writer failed");

    #[cfg(feature = "o3")]
    {
        // Check data length
        let tmp_file = get_only_file(tmp_dir.path()).expect("Temp File not found");
        // // copy to /dev/shm/sine.bin;
        // std::fs::copy(&tmp_file, "/dev/shm/sine.bin").expect("failed_to copy");
        let parser = V1parser::new(&tmp_file, false).expect("Could not parse tmp file");
        parser
            .resolve_m_overflow_idxs()
            .expect("Could not fix m_overflow.");
        assert_eq!(
            parser.data().expect("Data could not be obtained").len(),
            config.num_samples
        );

        // Check header
        let header = parser.header();
        log::debug!(
            "header_raw = {}",
            String::from_utf8(header.header_raw.to_vec()).expect("Could not read utf8")
        );

        assert_eq!(header.verbosity, config.verbosity);

        let zone_round = ZonedRound::new().smallest(jiff::Unit::Microsecond); // Python accuracy
        let file_start = header
            .start_time
            .clone()
            .expect("No start time")
            .round(zone_round)
            .expect("Rounding failed");
        let our_start = time_start.round(zone_round).expect("Rounding failed");
        let diff = file_start
            .until(&our_start)
            .expect("Difference not obtained")
            .total(jiff::Unit::Microsecond)
            .expect("Could not make as only microseconds");
        assert!(diff <= 1f64);

        assert_eq!(
            header.effective_decimation_factor(),
            NonZeroU64::new(1u64 + Configs::default().skip_num as u64)
                .expect("Could not read default")
        );

        // Check that the reduced phase is the same.
        let actual_reduced_phases = parser
            .reduced_phase()
            .expect("Could not get reduced phases");
        assert_eq!(actual_reduced_phases.len(), total_elements);

        use vhf_parse::unwrap_phase::VHFWordToUnwrappedPhaseByIter;
        let expected_reduced_phases: Vec<_> = signal_expected.to_unwrapped_phase(0).collect();

        // {
        //     log::info!("current_dir = {:?}", std::env::current_dir().unwrap());
        //     use plotters::prelude::*;
        //     let root = BitMapBackend::new("plot_me.png", (640, 480)).into_drawing_area();
        //     root.fill(&WHITE).expect("fill");
        //
        //     let mut chart = ChartBuilder::on(&root)
        //         .caption("Sine phase", ("sans-serif", 50).into_font())
        //         .margin(10)
        //         .x_label_area_size(30)
        //         .y_label_area_size(30)
        //         .build_cartesian_2d(0.0..1_000.1, -34_000.1..-31_000.1)
        //         .expect("build cart");
        //     chart
        //         .draw_series(LineSeries::new(
        //             actual_reduced_phases
        //                 .iter()
        //                 .cloned()
        //                 .enumerate()
        //                 .map(|(i, v)| (i as f64, v + 1.)),
        //             &BLUE,
        //         ))
        //         .expect("draw failed");
        //     chart
        //         .draw_series(LineSeries::new(
        //             expected_reduced_phases
        //                 .iter()
        //                 .cloned()
        //                 .enumerate()
        //                 .map(|(i, v)| (i as f64, v)),
        //             &RED,
        //         ))
        //         .expect("draw failed");
        //
        //     root.present().unwrap();
        //     log::info!("saved");
        // }

        // {
        //     use numpy::convert::ToPyArray;
        //     use pyo3::prelude::*;
        //     Python::with_gil(|py| {
        //         use std::env;
        //
        //         let sys_module = PyModule::import(py, "sys").unwrap();
        //         let executable_path = sys_module.getattr("path").unwrap();
        //
        //         // Print the executable path in Rust
        //         log::info!("Python path: {:?}", executable_path);
        //
        //         let expected: Vec<_> = expected_reduced_phases.clone();
        //         let expected = expected.to_pyarray(py);
        //
        //         log::info!("PYO3_PYTHON = {:?}", env::var("PYO3_PYTHON"));
        //         let mpl = Python::import(py, "matplotlib").unwrap();
        //         mpl.call_method1("use", ("TkAgg",)).unwrap();
        //         let plt = Python::import(py, "matplotlib.pyplot").unwrap();
        //         let subplots = plt.getattr("subplots").unwrap().call1((1, 1)).unwrap();
        //         let _fig = subplots.get_item(0).unwrap();
        //         let ax = subplots.get_item(1).unwrap();
        //         ax.call_method1("plot", (expected,)).unwrap();
        //         // fig.call_method1("show", (true,)).unwrap();
        //         plt.call_method1("savefig", ("output_plot.png",)).unwrap();
        //     });
        // }

        expected_reduced_phases
            .into_iter()
            .zip(actual_reduced_phases)
            .enumerate()
            .for_each(|(_, (e, a))| {
                assert_relative_eq!(e, a);
            });
    }

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
    config.save_to_file = true;
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

    let builder = config.file_writer().unwrap();
    matches!(builder.writer_type, Writers::V1(_));
    let mut writer = builder.with_start_time(time_start).build();

    debug_vhf
        .iter()
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v1_writer failed");

    assert_eq!(std::fs::read_dir(&tmp_dir).unwrap().count(), num_file);
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
    let builder = config.file_writer().unwrap();
    matches!(builder.writer_type, Writers::V1(_));
    let mut writer = builder.with_start_time(time_start).build();
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
        Err(e) => log::error!("Main loop occurred with error = {e:?}"),
    };

    // Check that this is a correct number of files.
    assert_eq!(std::fs::read_dir(&tmp_dir).unwrap().count(), num_file);

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}
