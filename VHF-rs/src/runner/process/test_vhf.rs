use super::consts::MMAP_PAGE_LEN;
use super::pages::*;
use super::*;
use crate::types::RawVHFWord;

use heapless::Deque;
use std::{
    matches,
    ops::Deref,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};
use tempfile::{NamedTempFile, TempDir};
use test_log::test;

// Create a VHF struct with false child thread "map_reader".
pub(super) fn debug_vhf_new(
    total_to_read: NonZeroUsize,
) -> (VHF, Arc<Mutex<Deque<MmapPage, DEQUE_CAP>>>, Arc<AtomicBool>) {
    let tmp_dir = TempDir::new().expect("Could not create temp_dir");
    let raw_tmp_file = NamedTempFile::new_in(tmp_dir).expect("Could not create temp file");

    let configuration = Config::default();
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
    let buffer = Arc::new(Mutex::new(Deque::new()));
    // Nonzero amount of time so that signal can also acquire Mutex
    let time_between_pages = Span::new()
        .try_milliseconds(1)
        .expect("Failed to make time_between_pages");
    let wake_mmap = Arc::new(RwLock::new(Instant::now()));

    (
        VHF {
            configuration: Box::new(configuration),
            handle,
            raw_handle,
            map_reader: Some(map_reader),
            engine_running: Arc::clone(&engine_running),
            vhf_stop: false,
            buffer_signal,
            buffer: Arc::clone(&buffer),
            time_between_pages,
            wake_mmap,
            total_pages_to_read: total_to_read,
        },
        buffer,
        engine_running,
    )
}

/// Pushes [pages::Page]s from slice into [VHF].buffer.
pub(super) fn push_arc_pages(
    buffer: Arc<Mutex<Deque<MmapPage, DEQUE_CAP>>>,
    data: impl Iterator<Item = RawVHFWord> + Send + 'static,
    sleep_between_pages: Duration,
    engine: Arc<AtomicBool>,
) -> Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("Unit Test: Buffer Page Creator".to_string())
        .spawn(move || {
            use itertools::Itertools;
            data.into_iter()
                .chunks(MMAP_PAGE_LEN)
                .into_iter()
                .map(|x| x.collect_array().unwrap())
                .map(Arc::new)
                .map(MmapPage::Page)
                .for_each(|x| {
                    'try_lock: loop {
                        if let Ok(mut buf) = buffer.try_lock()
                        // Obtain lock here so that parent thread still obtain lock.
                        {
                            buf.push_back(x).expect("Failed to push back onto buffer");
                            break 'try_lock;
                        } else {
                            log::debug!("failed to obtain lock to push onto buffer, trying again");
                            thread::sleep(sleep_between_pages / 100);
                            continue 'try_lock;
                        };
                    }
                    thread::sleep(sleep_between_pages);
                });
            // Set to false if Iterator has not already done so.
            engine.fetch_and(false, Ordering::AcqRel);
        })
        .map_err(Error::Io)
}

// Generate the zero-constant iterator on demand.
struct ZeroArr {
    total_len: usize,
    current_idx: AtomicUsize,
    engine_running: Arc<AtomicBool>,
}

impl Iterator for ZeroArr {
    type Item = RawVHFWord;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx.load(Ordering::Acquire) >= self.total_len {
            self.engine_running.fetch_and(false, Ordering::AcqRel);
            None
        } else {
            self.current_idx.fetch_add(1, Ordering::Relaxed);
            Some(0)
        }
    }
}

impl ZeroArr {
    fn new(total_len: usize, engine_running: Arc<AtomicBool>) -> Self {
        if total_len % MMAP_PAGE_LEN != 0 {
            log::warn!("ZeroArr did not receive an integer multiple of MMAP_PAGE_LEN");
        }
        Self {
            total_len,
            current_idx: AtomicUsize::new(0),
            engine_running,
        }
    }
}

/// We check if the iterator method does drop the Arc when .iter() has completed consuming.
#[test]
fn vhf_drops_arc() {
    let debug_vhf_total_len = 1;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN;
    let (debug_vhf, dbg_vhf_buffer, eng) =
        debug_vhf_new(NonZeroUsize::new(debug_vhf_total_len).unwrap());

    // We now add a weakpointer to the first object.
    let testing_page = Arc::new([0; MMAP_PAGE_LEN]);
    let to_drop = Arc::downgrade(&testing_page);

    // We now add data into the buffer.
    dbg_vhf_buffer
        .try_lock()
        .expect("Failed to lock buffer")
        .push_back(MmapPage::Page(testing_page))
        .expect("Failed to push_back testing page.");
    let push_arc_pages_thread = push_arc_pages(
        dbg_vhf_buffer,
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
                MmapPage::Page(x) => assert_eq!(x.deref(), &[0; MMAP_PAGE_LEN]),
                _ => unreachable!(),
            };
        });
    }

    // Check that content has been dropped.
    assert_eq!(to_drop.strong_count(), 0);

    push_arc_pages_thread.join().expect("Failed to join");
}

struct LinearArr {
    total_len: u64,
    current_idx: AtomicU64,
    engine_running: Arc<AtomicBool>,
}

impl Iterator for LinearArr {
    type Item = RawVHFWord;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx.load(Ordering::Acquire) >= self.total_len {
            self.engine_running.fetch_and(false, Ordering::AcqRel);
            None
        } else {
            Some(self.current_idx.fetch_add(1, Ordering::AcqRel))
        }
    }
}

impl LinearArr {
    fn new(total_len: usize, engine_running: Arc<AtomicBool>) -> Self {
        if total_len % MMAP_PAGE_LEN != 0 {
            log::warn!("LinearArr did not receive an integer multiple of MMAP_PAGE_LEN");
        }
        Self {
            total_len: total_len.try_into().unwrap(),
            current_idx: AtomicU64::new(0),
            engine_running,
        }
    }
}

/// We check that the VHF struct is yielding the correct windows with next.
#[test]
fn next_window_linear() {
    let debug_vhf_total_len = 5;
    let total_window_len = debug_vhf_total_len + VHF_MMAP_WINDOW_LEN - 1;
    let (debug_vhf, dbg_vhf_buffer, eng) =
        debug_vhf_new(NonZeroUsize::new(debug_vhf_total_len).unwrap());

    // Define the signal that we are testing for. (Use linear so its easier to determine.)
    let signal = LinearArr::new(total_window_len * MMAP_PAGE_LEN, eng.clone());

    // Add signal into pages. We now add data into the buffer.
    push_arc_pages(dbg_vhf_buffer, signal, Duration::default(), eng)
        .expect("push_arc_pages failed");

    let mut debug_vhf_iter = debug_vhf.iter();
    for _ in 0..debug_vhf_total_len {
        if let Some((idx, window)) = debug_vhf_iter.next() {
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

    let actual = debug_vhf_iter.next();
    if let Some(x) = actual.clone() {
        log::warn!(
            "Got page from vhf where none was expected: page[0]/MMAP_PAGE_LEN = {}; page[-1] = {}",
            x.1.first().unwrap().deref().first().unwrap() / (MMAP_PAGE_LEN as RawVHFWord),
            x.1.last().unwrap().deref().last().unwrap()
        );
    }
    assert!(actual.is_none());
}
