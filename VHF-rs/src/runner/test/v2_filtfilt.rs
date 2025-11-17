//! We aim to test that filtering works together with the v2 file writer.

use super::Config;
use super::consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN};
use super::fold::{
    StreamFold, StreamFoldOp,
    repr::{KernReprs, StreamFoldMapKernRepr},
};
use super::get_only_file;
use super::signals::{LinearPhaseArr, SineArr};
use super::writer::builder::Writers;
use super::{debug_vhf_new, push_arc_pages, required_window_pages};

use configparser::ini::Ini;
use jiff::Zoned;
use serde_json::json;
use std::f64::consts::TAU;
use std::num::NonZeroUsize;
use std::time::Duration;
use tempfile::TempDir;
use vhf_parse::{VHFparse, v2};

/// Test by considering the trivial filtfilt.
#[test]
#[ignore = "incomplete test"]
fn filtfilt_v2_file_trivial_filter() {
    let debug_vhf_total_len = 12 * VHF_MMAP_WINDOW_LEN;
    const PARAMS_STEP_BY: usize = VHF_MMAP_WINDOW_LEN.checked_sub(1).unwrap();
    // Ideally this should be derived from debug_vhf_conf, but we cannot due to circular
    // requirement; const to enforce at compile time.

    let total_window_len = required_window_pages(debug_vhf_total_len, PARAMS_STEP_BY);
    let total_elements = total_window_len * MMAP_PAGE_LEN;

    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    let mut debug_vhf_conf = Config::new(None).expect("Could not make empty config");
    debug_vhf_conf
        .with_config({
            let mut tmp = Ini::new();
            let _ = tmp
                .read(format!(
                    "[Board]
                    num_samples = {num_samples}
                    skip_num = 10 - 1
                    speed: low
                    encode: binary
                    vga_num = 0
                    vga_num_enable = False
                    filter_const = 0
                    filter_const_enable = False
                    v = 15

                    [Paths]
                    base_dir: .
                    save_dir: {save_dir}
                    save_to_file = True

                    [Phasemeter Details]
                    fibre: 1m",
                    num_samples = total_elements,
                    save_dir = tmp_dir
                        .path()
                        .to_str()
                        .expect("Tmp_dir could not be made into str"),
                ))
                .expect("Could not read toml");

            tmp
        })
        .expect("Could not populate from Ini");
    // No toml parsing for v2 ratio
    debug_vhf_conf.v2_overflow_to_data_ratio = Some(1.); // Is this excessive?
    log::info!("save_dir = {:?}", &debug_vhf_conf.save_dir);

    let filter_b = vec![1.]; // While scipy does reject this, this is the effective filter conceptually.
    let decimation_factor = unsafe { NonZeroUsize::new_unchecked(1) };
    // WARN: There is currently no means of exposing generating of filtfilt via
    // `Config::with_config`.
    debug_vhf_conf.stream_fold = {
        let (filt_b, filt_a) = (filter_b.clone(), [1.]);
        let filter_details = StreamFoldMapKernRepr {
            name: Some("boxcar".to_owned()),
            arg: json!({"M": 1, "sym": true}),
            value: Some(KernReprs::DiscreteFIRCoeff {
                b: filt_b.clone().into_boxed_slice(),
                zi: None,
            }),
        };

        StreamFold::filtfilt(decimation_factor, &filt_b, &filt_a, filter_details)
    };

    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let params_func = &debug_vhf_conf.stream_fold.func;
    assert_eq!(PARAMS_STEP_BY, params_func.step_by);
    assert_eq!(filter_b.len().div_ceil(MMAP_PAGE_LEN), params_func.pad);
    matches!(params_func.op, StreamFoldOp::Map(Some(_)));
    if let StreamFoldOp::Map(Some(map_arg)) = &params_func.op {
        assert_eq!(
            map_arg.effective_decimation, decimation_factor,
            "The trivial filter should result in effective_decimation of 1."
        );
        assert_eq!(
            map_arg.num_before_first_drop,
            decimation_factor.get().checked_sub(1).unwrap(),
            "The trivial filter should have no elements drop for file writing."
        );
    } else {
        panic!("func.op should have been Map(Some(_))!");
    }

    // Define the signal we are testing for.
    let signal_radius = 7000.;
    let initial_phase_offset = (i16::MAX as f64 - 70.9) * TAU;
    let phase_ang_freq = TAU / 755.876;
    // The signal must start within the m_offset = 0 range.
    let phase_ang_phi = -0.3;
    let phase_ampl = 883.3;
    let sinusoidal = SineArr::new(
        total_elements,
        eng.clone(),
        (
            phase_ampl,
            phase_ang_freq,
            phase_ang_phi,
            signal_radius,
            initial_phase_offset,
        ),
    );
    assert!(
        phase_ampl < initial_phase_offset / 4.,
        "Variation in the Φ(t) is too large for test!"
    );
    assert_eq!(PARAMS_STEP_BY, params_func.step_by);

    #[allow(unused)]
    let mut signal_expected = sinusoidal.clone(); // This will lose the engine

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread = push_arc_pages(
        dbg_vhf_sender,
        params_func.pad,
        sinusoidal,
        Duration::new(0, 100),
        eng,
    )
    .expect("push_arc_pages failed");

    let time_start = Zoned::now();
    let builder = debug_vhf_conf.file_writer().unwrap();
    matches!(builder.writer_type, Writers::V2Bin(_));
    let mut writer = builder.with_start_time(time_start.clone()).build();

    debug_vhf
        .iter()
        .step_by(params_func.step_by)
        .map(|x| (*params_func.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v2_writer failed");
    std::mem::drop(writer); // writer needs to be dropped to flush m_overflow_idx...

    // Open file through parser.
    let tmp_file = get_only_file(tmp_dir.path()).expect("Temp File not found");
    // std::fs::copy(&tmp_file, env::temp_dir().join("v2_sine.bin")).expect("failed_to copy");
    let mut parser = v2::VHFparser::new(&tmp_file, true).expect("Could not make v2 parser");

    // Test that the fold (as represented in the header) is empty.
    parser
        .get_header()
        .stream_fold
        .iter()
        .for_each(|fold| assert!(fold.is_empty()));

    // Test that the unwrapped phase is identical
    parser
        .resolve_m_overflow_idxs()
        .expect("Could not resolve m_overflow");
    parser
        .update_plot_timing(None, None, false)
        .expect("Could not update_plot_timing");
    log::debug!("parser = {parser:?}",);

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}
