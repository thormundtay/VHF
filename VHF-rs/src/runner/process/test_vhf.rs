use super::consts::MMAP_PAGE_LEN;
use super::pages::*;
use super::*;
use crate::types::RawVHFWord;

use heapless::Deque;
use std::{matches, ops::Deref};
use tempfile::{NamedTempFile, TempDir};
use test_log::test;

// Create a VHF struct with false child thread "map_reader".
pub(super) fn debug_vhf_new(total_to_read: NonZeroU64) -> VHF {
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    let raw_tmp_file =
        NamedTempFile::new_in(tmp_dir.into_path()).expect("Could not create temp file");

    let configuration = Config::default();
    let handle = {
        use std::os::fd::AsRawFd;
        raw_tmp_file.as_raw_fd()
    };
    let raw_handle = raw_tmp_file.into_file();
    let map_reader = thread::Builder::new()
        .name("false_reader".to_string())
        .spawn(|| {
            return Ok(());
        })
        .expect("map_reader could not be spawned.");
    let engine_running = Arc::new(AtomicBool::new(false));
    let buffer_signal = Arc::new(Condvar::new());
    let buffer = Arc::new(Mutex::new(Deque::new()));
    let time_between_pages = Span::new();
    let wake_mmap = Arc::new(RwLock::new(Instant::now()));

    VHF {
        configuration,
        handle,
        raw_handle,
        map_reader,
        engine_running,
        buffer_signal,
        buffer,
        time_between_pages,
        wake_mmap,
        total_to_read,
        windows_released: 0,
    }
}

/// Create [pages::Page]s from slice.
pub(super) fn create_arc_pages(data: &[RawVHFWord]) -> Vec<MmapPage> {
    let data_len = data.len();
    if data_len % MMAP_PAGE_LEN != 0 {
        log::warn!("create_arc_pages did not receive an integer multiple of MMAP_PAGE_LEN");
    }
    let mut result = Vec::with_capacity(data_len / MMAP_PAGE_LEN);

    use itertools::Itertools;
    data.iter()
        .chunks(MMAP_PAGE_LEN)
        .into_iter()
        .map(|x| x.copied().collect_array().unwrap())
        .map(Arc::new)
        .map(MmapPage::Page)
        .for_each(|x| result.push(x));

    result
}

/// We check if the iterator method does drop the Arc when .iter() has completed consuming.
#[test]
fn vhf_drops_arc() {
    let debug_vhf_total_len = 1;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let mut debug_vhf = debug_vhf_new(NonZeroU64::new(debug_vhf_total_len as u64).unwrap());

    // We now add a weakpointer to the first object.
    let testing_page = Arc::new([0; MMAP_PAGE_LEN]);
    let to_drop = Arc::downgrade(&testing_page);

    // We now add data into the buffer.
    if let Ok(mut buf) = debug_vhf.buffer.lock() {
        buf.push_back(MmapPage::Page(testing_page))
            .expect("Push back failed.");
        (1..total_window_len).for_each(|_| {
            create_arc_pages(&[0; MMAP_PAGE_LEN])
                .into_iter()
                .for_each(|x| {
                    buf.push_back(x).expect("Push back failed");
                })
        });
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    }

    // Pull out the first window, and check that content of the window is as expected.
    {
        let (_, first_window) = debug_vhf.next().unwrap();
        first_window.into_iter().for_each(|page| {
            assert!(matches!(page, MmapPage::Page(_)));
            match page {
                MmapPage::Page(x) => assert_eq!(x.deref(), &[0; MMAP_PAGE_LEN]),
                _ => unreachable!(),
            };
        });
    }

    // Check that content has been dropped.
    assert_eq!(to_drop.strong_count(), 0);
}

/// We check that the VHF struct is yielding the correct windows with next.
#[test]
fn next_window_linear() {
    let debug_vhf_total_len = 5;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN - 1;
    let mut debug_vhf = debug_vhf_new(NonZeroU64::new(debug_vhf_total_len as u64).unwrap());

    // Define the signal that we are testing for. (Use linear so its easier to determine.)
    let signal = 0..((total_window_len * MMAP_PAGE_LEN) as RawVHFWord);

    // Add signal into pages.
    // We now add data into the buffer.
    if let Ok(mut buf) = debug_vhf.buffer.lock() {
        let tmp_signal: Vec<_> = signal.clone().collect();
        create_arc_pages(&tmp_signal)
            .into_iter()
            .for_each(|x| buf.push_back(x).expect("Push back failed."));
    } else {
        log::error!("Could not get log in debug_vhf.buffer");
        panic!();
    };

    for _ in 0..debug_vhf_total_len {
        if let Some((idx, window)) = debug_vhf.next() {
            let expected_first: RawVHFWord = (idx * MMAP_PAGE_LEN).try_into().unwrap();
            let expected_last: RawVHFWord = ((idx + VHF_MMAP_WINDOW_LEN) * MMAP_PAGE_LEN - 1)
                .try_into()
                .unwrap();
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

    let actual = debug_vhf.next();
    if let Some(x) = actual.clone() {
        log::warn!(
            "Got page from vhf where none was expected: page[0]/MMAP_PAGE_LEN = {}; page[-1] = {}",
            x.1.first().unwrap().deref().first().unwrap() / (MMAP_PAGE_LEN as RawVHFWord),
            x.1.last().unwrap().deref().last().unwrap()
        );
    }
    assert!(actual.is_none());
}
