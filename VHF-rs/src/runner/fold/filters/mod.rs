//! All filter functions.
#![cfg_attr(feature = "doc-images", cfg_attr(all(),
    doc = embed_doc_image::embed_image!("window_defn", "src/runner/fold/filters/images/window_defn.png")
))]
#![cfg_attr(
    not(feature = "doc-images"),
    doc = "\n**Doc images not enabled**. Compile with feature `doc-images` to enable."
)]
//!
//! # Data from VHFIter
//!
//! We recall the definition for windows emitted by [VHFIter][crate::runner::VHFIter].
//! ![window_defn]
//! This is necessary as it is the first set of data taken in by a filter, before any successive
//! [fold operations].
//! The above is an example of 2 windows as emitted by [VHFIter][crate::runner::VHFIter].
//!
//! - <details><summary><code>Pad</code>: Number of overlapping pages.</summary>  
//!   This is the amount of overlap of data between windows of successive `vhf_iter_idx`.
//!
//!   - In the event that `vhf_iter_idx = 0`, the pages within `pad` are [empty
//!   pages][crate::runner::process::pages::MmapPage].  
//!   This has a value of `1` in the image.
//!   - For all other values of `vhf_iter_idx`, the pages within `pad` are the same page as the
//!   last `pad` number of pages from the previous window. This is as demonstrated by the identical
//!   `page_idx` and colouring in the provided image.
//!   - It is entirely possible that the final window of data has its final pages that are blank,
//!   as the VHF process has determined that sufficient data has been collected for the file
//!   writing.
//!
//!   The padding required can be specified by filter definition. It's value is given to take
//!   account the 'lookback' required by any filtering step. Further explanation on this will be
//!   given in the concrete example.
//!   </details>
//!
//!   As specified by [StreamFoldFunction::pad][super::StreamFoldFunction::pad].
//!
//! - <details><summary><code>VHF_MMAP_WINDOW_LEN</code>: The window length.</summary>
//!   In this image, it has the value of `20`.
//!
//!   It is possible to change this value at compile time if necessary.
//!   </details>
//!
//!   As specified by [VHF_MMAP_WINDOW_LEN][crate::runner::consts::VHF_MMAP_WINDOW_LEN].
//!
//! Now, to construct the filter, there are a few things to keep in mind:
//!
//! 1. The number of pages in a window emitted by [VHFIter][crate::runner::VHFIter].
//! 2. Not knowing if the current page being processed by the filter function is the final window,
//!    one must not discard otherwise valid data towards the tail end of the window (i.e.: the
//!    final page).
//! 3. Since almost every window is not the final window, how to extract data from the body of the
//!    window without duplication, accounting for the previous window's of data having been
//!    "exhuasted."
//!
//! [fold operations]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)
