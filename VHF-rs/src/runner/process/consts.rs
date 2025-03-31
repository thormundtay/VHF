//! All consts shared across [super::super::process] submodules.

/// This is the number of [crate::types::RawVHFWord] in one (kernel-sized) page emitted from the
/// MMap onto the heap.  
/// See [super::pages] in particular.
pub(super) const MMAP_PAGE_LEN: usize = 512;

// NOTE: HARDCODED! Currently used to determine the size of window being passed out from VHF.next()
// for mathematical transformation. This might need to increase if transforms really need to peer
// that far back. (Related?: https://github.com/rust-lang/rust/issues/60551)
/// The size of [item] in Iterator of [super::VHF].
///
/// [item]: super::VHF#impl-Iterator-for-VHF
pub(super) const VHF_MMAP_WINDOW_LEN: usize = 20;
