//! This file is to test if [super::super::Writer] works, particularly so in the context of v2 file
//! writer.

use super::super::fold::StreamFoldOp;
use super::super::writer::builder::Writers;
use super::Config;
use super::consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN};
use super::get_only_file;
use super::signals::{LinearPhaseArr, SineArr};
use super::{debug_vhf_new, push_arc_pages, required_window_pages};

use approx::{AbsDiffEq, RelativeEq};
use configparser::ini::Ini;
use jiff::Zoned;
#[allow(unused_imports)]
use std::env;
use std::f64::consts::TAU;
use std::hint::unreachable_unchecked;
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
    log::info!("save_dir = {:?}", &debug_vhf_conf.save_dir);

    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let params_func = &debug_vhf_conf.stream_fold.func;
    matches!(params_func.op, StreamFoldOp::Map(None));
    assert_eq!(PARAMS_STEP_BY, params_func.step_by);

    // We want to force an unwrapping to occur at least once.
    assert!(total_elements > u16::MAX as usize + 3);
    let signal_radius = 7000.;
    let initial_reduced_phase = i16::MAX as f64 - 240.9;
    let reduced_phase_gradient = 0.21;
    let linear = LinearPhaseArr::new(
        total_elements,
        (signal_radius, initial_reduced_phase, reduced_phase_gradient),
        eng.clone(),
    );

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread = push_arc_pages(
        dbg_vhf_sender,
        params_func.pad,
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
        .step_by(params_func.step_by)
        .map(|x| (*params_func.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v2_writer failed");
    std::mem::drop(writer); // writer needs to be dropped to flush m_overflow_idx...

    // Open file through parser.
    let tmp_file = get_only_file(tmp_dir.path()).expect("Temp File not found");
    // std::fs::copy(&tmp_file, env::temp_dir().join("v2_linear.bin")).expect("failed_to copy");
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

    let raw_data = parser.data().expect("Could not get raw data");
    assert_eq!(raw_data.len(), total_elements);

    // Manually log all overflow indices
    let (expected_overflow_idx, expected_overflow_sign) = {
        let mut idxs = Vec::new();
        let mut signs = Vec::new();
        let _ = raw_data.iter().enumerate().fold(
            {
                let word = VHFWord::from(*raw_data.first().unwrap());
                let IQMTriplet(_, _, m) = word.into();
                m
            },
            |prev_m, (idx, &curr_data)| {
                let word = VHFWord::from(curr_data);
                let IQMTriplet(_i, _q, m) = word.into();
                if m.abs_diff(prev_m) > u16::MAX / 4 {
                    // let IQMTriplet(pi, pq, pm)=
                    //     VHFWord::from(*raw_data.iter().nth(idx - 1).unwrap()).into();
                    // log::debug!(
                    //     "Overflow on data[idx-1] = IQM({pi:>6}, {pq:>6}, {pm:>6}) -- idx = {idx}: IQM({i:>6}, {q:>6}, {m:>6})",
                    // );
                    idxs.push(idx);
                    signs.push(match m.cmp(&prev_m) {
                        std::cmp::Ordering::Less => 1,
                        std::cmp::Ordering::Greater => -1,
                        std::cmp::Ordering::Equal => unsafe { unreachable_unchecked() },
                    })
                };

                m
            },
        );
        (idxs, signs)
    };
    // Point is to test situation where there is at least 1 element written into m_overflow!
    assert!(!expected_overflow_idx.is_empty());

    // Check m_mgr
    let m_mgr = parser.get_m_mgr().expect("manifold manager not found");
    let actual_idx: Vec<_> = m_mgr.get_delta_idx().collect();
    assert_eq!(actual_idx.len(), expected_overflow_idx.len());
    assert!(
        actual_idx
            .into_iter()
            .zip(expected_overflow_idx)
            .all(|(a, e)| a == e)
    );
    assert!(
        m_mgr
            .get_delta_signs()
            .zip(expected_overflow_sign)
            .all(|(a, e)| a as i16 == e)
    );

    // Check reduced phases
    let reduced_phase = parser.reduced_phase().expect("Could not get reduced_phase");
    assert!(
        reduced_phase
            .first()
            .unwrap()
            .abs_diff_eq(&initial_reduced_phase, 1e-5)
    );
    reduced_phase
        .windows(2)
        .into_iter()
        .all(|w| (w[1]).abs_diff_eq(&w[0], 1e-5));

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}

/// Test for when the v2 file does not have enough allocated m_overflow file buffer.
///
/// This also implicitly tests the v2 file reader if it is able to handle the case where there
/// exists a m_overflow buffer but it is filled
#[test]
fn writes_correct_v2_file_insufficient() {
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
    log::info!("save_dir = {:?}", &debug_vhf_conf.save_dir);

    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let params_func = &debug_vhf_conf.stream_fold.func;
    matches!(params_func.op, StreamFoldOp::Map(None));
    assert_eq!(PARAMS_STEP_BY, params_func.step_by);

    let signal_radius = 7000.;
    let initial_phase_offset = (i16::MAX as f64 - 70.9) * TAU;
    let phase_ang_freq = TAU / 755.876;
    let phase_ang_phi = 0.;
    let phase_ampl = 14883.3;
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
    assert_eq!(PARAMS_STEP_BY, params_func.step_by);

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

    let raw_data = parser.data().expect("Could not get raw data");
    assert_eq!(raw_data.len(), total_elements);

    // Manually log all overflow indices
    let (expected_overflow_idx, expected_overflow_sign) = {
        let mut idxs = Vec::new();
        let mut signs = Vec::new();
        let _ = raw_data.iter().enumerate().fold(
            {
                let word = VHFWord::from(*raw_data.first().unwrap());
                let IQMTriplet(_, _, m) = word.into();
                m
            },
            |prev_m, (idx, &curr_data)| {
                let word = VHFWord::from(curr_data);
                let IQMTriplet(_i, _q, m) = word.into();
                if m.abs_diff(prev_m) > u16::MAX / 4 {
                    idxs.push(idx);
                    signs.push(match m.cmp(&prev_m) {
                        std::cmp::Ordering::Less => 1,
                        std::cmp::Ordering::Greater => -1,
                        std::cmp::Ordering::Equal => unsafe { unreachable_unchecked() },
                    })
                };

                m
            },
        );
        (idxs, signs)
    };
    // Point is to test situation where there is at least 1 element written into m_overflow!
    assert!(!expected_overflow_idx.is_empty());

    // Check m_mgr
    let m_mgr = parser.get_m_mgr().expect("manifold manager not found");
    let actual_idx: Vec<_> = m_mgr.get_delta_idx().collect();
    assert_eq!(actual_idx.len(), expected_overflow_idx.len());
    assert!(
        actual_idx
            .into_iter()
            .zip(expected_overflow_idx)
            .all(|(a, e)| a == e)
    );
    assert!(
        m_mgr
            .get_delta_signs()
            .zip(expected_overflow_sign)
            .all(|(a, e)| a as i16 == e)
    );

    // Check reduced phases
    let reduced_phase = parser.reduced_phase().expect("Could not get reduced_phase");
    let expected_zeroth_phase = (0f64/*Zero is the index*/)
        .mul_add(phase_ang_freq, phase_ang_phi)
        .sin()
        .mul_add(phase_ampl, initial_phase_offset);
    log::info!(
        "expected_zeroth_phase = {expected_zeroth_phase:?}; phase[0]*TAU = {:?}",
        reduced_phase.first().map(|rp| rp * TAU).unwrap()
    );
    assert!(
        reduced_phase
            .first()
            .map(|rp| rp * TAU)
            .unwrap()
            .relative_eq(&expected_zeroth_phase, 1e-6, 1e-6)
    );
    reduced_phase
        .windows(2)
        .into_iter()
        .all(|w| (w[1]).abs_diff_eq(&w[0], 1e-5));

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}

/// Test for when the v2 file no allocated m_overflow file buffer.
///
/// This also implicitly tests the v2 file reader if it is able to handle the case where there
/// does not exists a m_overflow buffer.
#[test]
fn writes_correct_v2_file_zero() {
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
    // No toml parsing for v2 ratio yet
    debug_vhf_conf.v2_overflow_to_data_ratio = Some(0.0);
    log::info!("save_dir = {:?}", &debug_vhf_conf.save_dir);

    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    let params_func = &debug_vhf_conf.stream_fold.func;
    matches!(params_func.op, StreamFoldOp::Map(None));
    assert_eq!(PARAMS_STEP_BY, params_func.step_by);

    // We want to force an unwrapping to occur at least once.
    assert!(total_elements > u16::MAX as usize + 3);
    let signal_radius = 7000.;
    let initial_reduced_phase = i16::MAX as f64 - 240.9;
    let reduced_phase_gradient = 0.21;
    let linear = LinearPhaseArr::new(
        total_elements,
        (signal_radius, initial_reduced_phase, reduced_phase_gradient),
        eng.clone(),
    );

    // Add signal into pages. We now add data into the buffer.
    let push_arc_pages_thread = push_arc_pages(
        dbg_vhf_sender,
        params_func.pad,
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
        .step_by(params_func.step_by)
        .map(|x| (*params_func.func)(x))
        .try_for_each(|write_block| writer.write_data(write_block))
        .expect("Writing to v2_writer failed");
    std::mem::drop(writer); // writer needs to be dropped to flush m_overflow_idx...

    // Open file through parser.
    let tmp_file = get_only_file(tmp_dir.path()).expect("Temp File not found");
    // std::fs::copy(&tmp_file, env::temp_dir().join("v2_zero.bin")).expect("failed_to copy");
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

    let raw_data = parser.data().expect("Could not get raw data");
    assert_eq!(raw_data.len(), total_elements);

    // Manually log all overflow indices
    let (expected_overflow_idx, expected_overflow_sign) = {
        let mut idxs = Vec::new();
        let mut signs = Vec::new();
        let _ = raw_data.iter().enumerate().fold(
            {
                let word = VHFWord::from(*raw_data.first().unwrap());
                let IQMTriplet(_, _, m) = word.into();
                m
            },
            |prev_m, (idx, &curr_data)| {
                let word = VHFWord::from(curr_data);
                let IQMTriplet(_i, _q, m) = word.into();
                if m.abs_diff(prev_m) > u16::MAX / 4 {
                    idxs.push(idx);
                    signs.push(match m.cmp(&prev_m) {
                        std::cmp::Ordering::Less => 1,
                        std::cmp::Ordering::Greater => -1,
                        std::cmp::Ordering::Equal => unsafe { unreachable_unchecked() },
                    })
                };

                m
            },
        );
        (idxs, signs)
    };
    // Point is to test situation where there is at least 1 element written into m_overflow!
    assert!(!expected_overflow_idx.is_empty());

    // Check m_mgr
    let m_mgr = parser.get_m_mgr().expect("manifold manager not found");
    let actual_idx: Vec<_> = m_mgr.get_delta_idx().collect();
    assert_eq!(actual_idx.len(), expected_overflow_idx.len());
    assert!(
        actual_idx
            .into_iter()
            .zip(expected_overflow_idx)
            .all(|(a, e)| a == e)
    );
    assert!(
        m_mgr
            .get_delta_signs()
            .zip(expected_overflow_sign)
            .all(|(a, e)| a as i16 == e)
    );

    // Check reduced phases
    let reduced_phase = parser.reduced_phase().expect("Could not get reduced_phase");
    assert!(
        reduced_phase
            .first()
            .unwrap()
            .abs_diff_eq(&initial_reduced_phase, 1e-5)
    );
    reduced_phase
        .windows(2)
        .into_iter()
        .all(|w| (w[1]).abs_diff_eq(&w[0], 1e-5));

    tmp_dir.close().expect("Could not close temp_dir.");
    push_arc_pages_thread.join().expect("Failed to join");
}
