//! This file is to test if [super::super::Writer] works, particularly so in the context of v2 file
//! writer.

use super::super::fold::StreamFoldOp;
use super::super::writer::builder::Writers;
use super::Config;
use super::consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN};
use super::get_only_file;
use super::signals::LinearPhaseArr;
use super::{debug_vhf_new, push_arc_pages};

use approx::AbsDiffEq;
use configparser::ini::Ini;
use jiff::Zoned;
use std::num::NonZeroUsize;
use std::time::Duration;
use tempfile::TempDir;
use test_log::test;
use vhf_common::data_types::IQMTriplet;
use vhf_parse::v2;
use vhf_parse::{VHFWord, VHFparse};

/// Tests just amount of written data.
#[test]
fn writes_correct_v2_file_basic() {
    let debug_vhf_total_len = 12 * VHF_MMAP_WINDOW_LEN;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
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
    log::info!("save_dir = {:?}", &debug_vhf_conf.save_dir);

    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let params = &debug_vhf_conf.stream_fold;
    matches!(params.op, StreamFoldOp::Map(None));

    // We want to force an unwrapping to occur at least once.
    assert!(total_elements > u16::MAX as usize + 3);
    let signal_radius = 7000.;
    // let initial_reduced_phase = i16::MAX as f64 - 240.9;
    let initial_reduced_phase = 0.;
    let reduced_phase_gradient = 0.21;
    let linear = LinearPhaseArr::new(
        total_elements,
        (signal_radius, initial_reduced_phase, reduced_phase_gradient),
        eng.clone(),
    );

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread = push_arc_pages(
        dbg_vhf_sender,
        params.pad,
        linear,
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
        .step_by(params.step_by)
        .map(|x| (*params.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v2_writer failed");
    std::mem::drop(writer); // writer needs to be dropped to flush m_overflow_idx...

    // Test that the unwrapped phase is identical
    let tmp_file = get_only_file(tmp_dir.path()).expect("Temp File not found");
    let mut parser = v2::VHFparser::new(&tmp_file, true).expect("Could not make v2 parser");
    parser
        .resolve_m_overflow_idxs()
        .expect("Could not resolve m_overflow");
    parser
        .update_plot_timing(None, None, false)
        .expect("Could not update_plot_timing");
    log::debug!("parser = {parser:?}",);

    let raw_data = parser.data().expect("Could not get raw data");
    // Manually log all overflow indices
    raw_data.iter().enumerate().fold(
        {
            let word = VHFWord::from(*raw_data.first().unwrap());
            let IQMTriplet(_, _, m) = word.into();
            m
        },
        |prev_m, (idx, &curr_data)| {
            let word = VHFWord::from(curr_data);
            let IQMTriplet(i, q, m) = word.into();
            if m.abs_diff(prev_m) > u16::MAX / 4 {
                let IQMTriplet(pi, pq, pm)=
                    VHFWord::from(*raw_data.iter().nth(idx - 1).unwrap()).into();
                log::debug!(
                    "Overflow on data[idx-1] = IQM({pi:>6}, {pq:>6}, {pm:>6}) -- idx = {idx}: IQM({i:>6}, {q:>6}, {m:>6})",
                );
            };

            m
        },
    );

    let reduced_phase = parser.reduced_phase().expect("Could not get reduced_phase");
    assert_eq!(*reduced_phase.first().unwrap(), initial_reduced_phase);
    reduced_phase
        .windows(2)
        .into_iter()
        .all(|w| (w[1]).abs_diff_eq(&w[0], 1e-5));

    // Save to external
    // std::fs::copy(&tmp_file, "/dev/shm/v2_linear.bin").expect("failed_to copy");

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}
