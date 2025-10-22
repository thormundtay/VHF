//! All filter functions.
#![cfg_attr(feature = "doc-images", cfg_attr(all(),
    doc = embed_doc_image::embed_image!("window_defn", "src/runner/fold/filters/images/window_defn.png"),
    doc = embed_doc_image::embed_image!("filter_wo_decimation", "src/runner/fold/filters/images/filter_wo_decimation.png"),
    doc = embed_doc_image::embed_image!("final_window_case_a", "src/runner/fold/filters/images/final_window_case_a.png"),
    doc = embed_doc_image::embed_image!("final_window_case_b", "src/runner/fold/filters/images/final_window_case_b.png"),
))]
#![cfg_attr(
    not(feature = "doc-images"),
    doc = "\n**Doc images not enabled**. Compile with feature `doc-images` to enable."
)]
//!
//! # Data from VHFIter
//!
//! We recall the definition for windows emitted by [VHFIter][crate::runner::VHFIter], called
//! `VHF_MMAP_WINDOW`.
//! ![window_defn]
//! This is necessary as it is the first set of data taken in by a filter, before any successive
//! [fold operations].
//! The above is an example of 2 windows as emitted by [VHFIter][crate::runner::VHFIter]. Windows
//! are composed of contiguous [Pages][crate::runner::process::pages::MmapPage].
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
//!    The number of pages is determined at configuration time, as set
//!    [here][crate::runner::process::VHF::total_pages_to_read].
//! 3. Since almost every window is not the final window, how to extract data from the body of the
//!    window without duplication, accounting for the previous window's of data having been
//!    "exhuasted."
//!
//! # Constructing appropriate filters: A worked example with Lfilter
//!
//! We use a single pass FIR Lfilter as an example to elaborate further on points 2 and 3.
//!
//! To ensure consistent understanding, we remind what a FIR Lfilter does.  
//! A FIR filter of order `P` is a (`P+1`)-list of coefficients, often denoted as `[b_0, b_1, ...,
//! b_P]`. Thereafter for a discrete input signal `x[n]` (indexed by `n`), the resulting signal
//! `y[n]` is then given by
//! ```custom,{class=language-latex}
//! \[ y[n] = \sum_{i=0}^N b_i x[n-i]. \]
//! ```
//! For brevity, no initial conditions associated with the [typical
//! Lfilter][crate::runner::fold::repr::KernReprs::DiscreteFIRCoeff] will be described.
//!
//! Next, the filter operates on the window of data with the intent of [decimation]. Up to some
//! properties of the filter, the filtering process aims to preserve some spectral content
//! accurately during the down-sampling. The "decimation factor", say `d` is thus the every `d`-th
//! data point that is kept after the filter is applied onto every data point.
//!
//! Let us diagramatically represent the filtering process across a data slice straddling the
//! boundary of two non-empty pages.
//!
//! ![filter_wo_decimation]
//!
//! As a FIR Lfilter is causal, the diagram shows the result of each convolved window as being in
//! the index of the final convolution window's index. To avoid unnecessary confusion, we describe
//! the convolution window against the filter as being a context window. (Furthermore, context is a
//! better nomenclature, as not everything that will be done, will necessarily be convolution.)
//!
//! Here, the FIR Lfilter convolution kernel of order 6 is applied for every valid index, as is
//! represented by the context window repeated whilst sliding to the right.  
//! For the convolution kernel in a fixed index, data elements are correspondingly multiplied with
//! the kernel elements, before being summed to obtain the final value in the `output` array. This
//! is diagramatically represented by the red data point in the output and how it's derived from
//! the related Lfilter convolution kernel and data points. Note that the index written into the
//! `output` array is true up to a constant offset, whose value is dependent on how the convolution
//! kernel handles the data in page 0 index ~0. This is discussed in greater detail later.
//!
//! Duly note that the `(VHF_)MMAP_WINDOW` taken in for filtering at this step is done independently
//! of every other filtering step. As such, the only valid assumption is that there exists previous
//! `MMAP_WINDOW` as processed by the same function (in a possibly different thread). There is no
//! guarantee that there exists a next `MMAP_WINDOW`, as it is entirely possible that the current
//! `MMAP_WINDOW` is the last `MMAP_WINDOW` emitted by `VHF`. As such, output from the context
//! convolution process on overlapping `pages` towards the back of the `MMAP_WINDOW` should be
//! emitted into `out` for file writing. Exercise appropriate care if there final pages of
//! `MMAP_WINDOW` are [End][crate::runner::process::pages::MmapPage::End], which indicates that the
//! current `MMAP_WINDOW` for filtering is indeed the final `MMAP_WINDOW`.  
//! In other words, observe the definition of `VHF_MMAP_WINDOW` as provided in [the previous
//! section][sec:data_from_vhf_iter] as is provided in a streaming fashion, and determine how one
//! should process the data with as minimal assumptions as possible. Diagramatically, this would
//! correspond to  
//! - Case 1: ![final_window_case_a]
//!   Here, the window has a page that is explicitly declared as
//!   [empty][crate::runner::process::pages::MmapPage::End] towards the back of the window, and
//!   thus is trivial.
//! - Case 2: ![final_window_case_b]
//!   Suppose that the filter is acting on the window with `vhf_iter_idx=1`, where all pages are
//!   filled. There is no guarantee that there exists a window with `vhf_iter_idx=2`, and thus, all
//!   data located within the end `Pad`ding should be processed and written to file.
//!   Note!: This principle can be violated at your own risk of coordination with
//!   [VHFIter][crate::runner::VHFIter].
//!
//! [fold operations]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)
//! [decimation]: https://en.wikipedia.org/wiki/Downsampling_(signal_processing)
//! [sec:data_from_vhf_iter]: #data-from-vhfiter
