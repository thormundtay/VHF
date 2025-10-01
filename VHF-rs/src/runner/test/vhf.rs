use super::Config;
use super::consts::{MMAP_PAGE_LEN, VHF_MMAP_WINDOW_LEN};
use super::pages::MmapPage;
use super::signals::{LinearArr, ZeroArr};
use super::{DEQUE_CAP, VHF};
use crate::{Error, Result};
use vhf_common::data_types::RawVHFWord;

use heapless::Deque;
use jiff::Span;
use std::{
    cell::RefCell,
    matches,
    num::NonZeroUsize,
    ops::Deref,
    rc::Rc,
    sync::{
        Arc, Condvar, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tempfile::{NamedTempFile, TempDir};
use test_log::test;

/// Rounds up to the appropriate number of pages so that mocked engine pushes all pages through
/// step_by iterator.
pub(super) fn required_window_pages(intended_pages: usize, step_by: usize) -> usize {
    assert!(step_by < VHF_MMAP_WINDOW_LEN);
    let a = VHF_MMAP_WINDOW_LEN - step_by; // 20 (VHF_MMAP_WINDOW_LEN) - 19 (STEP_BY) = 1 (OVERLAP EXPECTED)

    (VHF_MMAP_WINDOW_LEN.max(intended_pages) - a).div_ceil(step_by) * step_by
}

/// Create a VHF struct with false child thread "map_reader".
///
/// Arguments:
/// - configuration: Basic properties of the mock VHF engine being expected.
/// - total_to_read: number of pages that the false map_reader should be reading.
///
/// Returns:
/// - VHF: Iterate to get MmapPages for processing.
/// - SyncSender: Used to push pages from some signal generator into VHF.
/// - Engine: Used for coordinate VHF with the signal generator.
pub(super) fn debug_vhf_new<'a>(
    configuration: &'a Config,
    total_to_read: NonZeroUsize,
) -> (VHF<'a>, SyncSender<MmapPage>, Arc<AtomicBool>) {
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    let raw_tmp_file = NamedTempFile::new_in(tmp_dir).expect("Could not create temp file");

    let configuration = configuration.build_board_config().unwrap();
    let handle = {
        use std::os::fd::AsRawFd;
        raw_tmp_file.as_raw_fd()
    };
    let raw_handle = raw_tmp_file.into_file();
    let map_reader = thread::Builder::new()
        .name("false_reader".to_string())
        .spawn(|| Ok(()))
        .expect("map_reader could not be spawned.");
    let engine_running = Arc::new(AtomicBool::new(true));
    let buffer_signal = Arc::new(Condvar::new());
    let (buffer_sender, buffer_receive) = sync_channel(DEQUE_CAP);
    let buffer = Rc::new(RefCell::new(Deque::new()));
    // Nonzero amount of time so that signal can also acquire Mutex
    let time_between_pages = Span::new()
        .try_milliseconds(1)
        .expect("Failed to make time_between_pages");
    let wake_mmap = Arc::new(RwLock::new(Instant::now()));

    (
        VHF {
            configuration,
            handle,
            raw_handle,
            map_reader: Some(map_reader),
            engine_running: Arc::clone(&engine_running),
            vhf_stop: false,
            buffer_signal,
            buffer_receive,
            buffer,
            time_between_pages,
            wake_mmap,
            total_pages_to_read: total_to_read,
        },
        buffer_sender,
        engine_running,
    )
}

/// Pushes [pages::Page]s from slice into [VHF].buffer.
/// Note that this bypasses the MmapReader.
///
/// Arguments:
/// - buffer_sender: Sender end of channel for pushing into [VHF].
/// - empty_pages: Number of empty pages as given by [super::fold::StreamFold].
/// - data: Any Iterator of [RawVHFWord].
/// - sleep_between_pages: Time spent sleeping between each page push.
/// - engine: Synchronization used to determine if VHF is running.
pub(super) fn push_arc_pages(
    buffer_sender: SyncSender<MmapPage>,
    empty_pages: usize,
    data: impl Iterator<Item = RawVHFWord> + Send + 'static,
    sleep_between_pages: Duration,
    engine: Arc<AtomicBool>,
) -> Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("Unit Test: Buffer Page Creator".to_string())
        .spawn(move || {
            use itertools::Itertools;
            (0..empty_pages).for_each(|_| {
                buffer_sender
                    .send(MmapPage::Empty)
                    .expect("Failed to to push empty page onto buffer.");
            });
            data.into_iter()
                .chunks(MMAP_PAGE_LEN)
                .into_iter()
                .map(|x| x.collect_array().unwrap())
                .map(Arc::new)
                .map(MmapPage::Page)
                .for_each(|x| {
                    buffer_sender
                        .send(x)
                        .expect("Failed to push back onto buffer");
                    thread::sleep(sleep_between_pages);
                });
            // Set to false if Iterator has not already done so.
            engine.fetch_and(false, Ordering::AcqRel);
        })
        .map_err(Error::Io)
}

/// We check if the iterator method does drop the Arc when .iter() has completed consuming.
#[test]
fn vhf_drops_arc() {
    let debug_vhf_total_len = 1;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let debug_vhf_conf = Config::default();
    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    // We now add a weakpointer to the first object.
    let testing_page = Arc::new([RawVHFWord::from(0); MMAP_PAGE_LEN]);
    let to_drop = Arc::downgrade(&testing_page);

    // We now add data into the buffer.
    dbg_vhf_sender
        .send(MmapPage::Page(testing_page))
        .expect("Failed to push_back testing page.");
    let push_arc_pages_thread = push_arc_pages(
        dbg_vhf_sender,
        0,
        ZeroArr::new((total_window_len - 1) * MMAP_PAGE_LEN, eng.clone()),
        Duration::default(),
        eng,
    )
    .expect("push_arc_pages failed");

    // Pull out the first window, and check that content of the window is as expected.
    {
        let (_, first_window) = debug_vhf.iter().next().unwrap();
        first_window.into_iter().for_each(|page| {
            assert!(matches!(page, MmapPage::Page(_)));
            match page {
                MmapPage::Page(x) => assert_eq!(x.deref(), &[RawVHFWord::from(0); MMAP_PAGE_LEN]),
                _ => unreachable!(),
            };
        });
    }

    // Check that content has been dropped.
    assert_eq!(to_drop.strong_count(), 0);

    push_arc_pages_thread.join().expect("Failed to join");
}

/// We check that the VHF struct is yielding the correct windows with next.
#[test]
fn next_window_linear() {
    let debug_vhf_total_len = 5;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN - 1;
    let debug_vhf_conf = Config::default();
    let (debug_vhf, dbg_vhf_sender, eng) = debug_vhf_new(
        &debug_vhf_conf,
        NonZeroUsize::new(debug_vhf_total_len).unwrap(),
    );

    // Define the signal that we are testing for. (Use linear so its easier to determine.)
    let signal = LinearArr::new(total_window_len * MMAP_PAGE_LEN, eng.clone());

    // Add signal into pages. We now add data into the buffer.
    push_arc_pages(
        dbg_vhf_sender,
        debug_vhf_conf
            .build_board_config()
            .expect("Could not build board config")
            .stream_fold_parameters()
            .pad,
        signal,
        Duration::default(),
        eng,
    )
    .expect("push_arc_pages failed");

    let mut debug_vhf_iter = debug_vhf.iter();
    for _ in 0..debug_vhf_total_len {
        if let Some((idx, window)) = debug_vhf_iter.next() {
            let expected_first: u64 = (idx * MMAP_PAGE_LEN).try_into().unwrap();
            let expected_last: u64 = ((idx + VHF_MMAP_WINDOW_LEN) * MMAP_PAGE_LEN - 1)
                .try_into()
                .unwrap();

            let expected_first = RawVHFWord::from(expected_first);
            let expected_last = RawVHFWord::from(expected_last);
            assert_eq!(
                *window.first().unwrap().deref().first().unwrap(),
                expected_first
            );

            assert_eq!(
                *window.last().unwrap().deref().last().unwrap(),
                expected_last,
            );
        } else {
            panic!("There should be a window.");
        }
    }

    let actual = debug_vhf_iter.next();
    if let Some(x) = actual.clone() {
        log::warn!(
            "Got page from vhf where none was expected: page[0]/MMAP_PAGE_LEN = {}; page[-1] = {:?}",
            x.1.first().unwrap().deref().first().unwrap().as_u64() / (MMAP_PAGE_LEN as u64),
            x.1.last().unwrap().deref().last().unwrap()
        );
    }
    assert!(actual.is_none());
}
