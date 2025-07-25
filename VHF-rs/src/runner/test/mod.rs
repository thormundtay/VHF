//! Tests here often aim to test the correctness of the file writer, which often also tests their
//! corresponding parsers. The strategy taken up is by building up the through the testing of
//! various components.

use crate::{Error, Result};
use std::path::PathBuf;

// Let all tests within each mod reduce by 1 super by scoping as if they were in [crate::runner].
use super::*;

/// Various signals to hijack into MmapReader.
pub mod signals;

/// Test [super::super::process::VHF] pushing "ingesting from FPGA" before pushing out as an Iter.
/// This is mostly done so by spoofing the MMapReader thread.
mod vhf;
use vhf::{debug_vhf_new, push_arc_pages};

/// Assuming pages pushed out as an iterator are correct, we now test that the processing of this
/// pages prior to file writing are done to expectation. This is primarily for the comparatively
/// simpler folds done in [super::super::fold].
mod vhf_step_fold;

/// Specifically to v1 file writer, test if the file writing is sound (up to some concerns).
/// As the parser is written in Python, please invoke the feature flag to test if the data as
/// written is consistent with the parser.
mod v1_write;

/// For a temp dir, check that there has only been a single file in it. Thereafter, yield the path
/// to it.
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

/// In the event that Python integration is invoked for the testing, ensure that the environment is
/// to expectation.
#[cfg(feature = "o3")]
mod o3_test_setup {
    use std::env;
    use test_log::test;

    /// This test should not occur if o3 feature is not activated in the test.
    #[test]
    fn correct_pwd() {
        pyo3::prepare_freethreaded_python();
        log::info!("PYTHONPATH = {:?}", env::var("PYTHONPATH"));
        assert!(vhf_parse::v1_python::test_import())
    }
}

/// Specifically to v2 file writer, test if the file writing is sound (up to some concerns).
mod v2_write;
