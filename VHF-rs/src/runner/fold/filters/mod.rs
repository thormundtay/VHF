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

use super::{
    super::fold::MapArg,
    StreamFoldFunction, StreamFoldOp, StreamFoldRepr,
    repr::{
        Filter, StreamFoldMapFilterRepr, StreamFoldMapKernRepr, StreamFoldMapRepr, StreamFoldOpRepr,
    },
};
use crate::parser::consts::M_OVERFLOW;
use crate::runner::VHFIter;
use crate::runner::consts::{MMAP_PAGE_LEN as PAGE_LEN, VHF_MMAP_WINDOW_LEN as WINDOW_LEN};
use crate::runner::process::pages::MmapPage as Page;
use crate::runner::writer::WriteBlock;
use ndarray::{Array1, ArrayView1};
use sci_rs::signal::filter::prelude::FftProcessor;
use std::cmp::Ordering;
use std::hint::unreachable_unchecked;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use vhf_common::data_types::{IQMTriplet, Polar};
use vhf_parse::VHFWord;

/// Filter functions denoting that all phase has the same type as ReducedPhase, but this is scaled
/// to the unit-circle instead of multiples of `m` (or equivalently, wavelength).
type Phase = vhf_parse::ReducedPhase;
/// The magnitude component of an unwrapped RawVHFWord.
///
/// Specific to just filtering functions for now.
type Radius = f64; /* This is just that we requires floats. */

/// Return type specifically for [unwrap_phases_in_window].
#[derive(Default)]
struct PolarAndOverflowRaw {
    /// Unwrapped Phase (i.e.: i16 limitation of m has been accounted for.)
    ///
    /// This is not normalized by 2π, i.e.: The phase here is not reduced (aka wavelength).
    unwrapped_phase: Vec<Phase>,
    /// Magnitude of Polar representation of [vhf_parse::VHFWord].
    radius: Vec<Radius>,
    // TODO: Tie the types here with the inner-types of MOverflowRaw.
    #[allow(dead_code)]
    overflow_raw: Option<(Vec<usize>, Vec<i8>)>,
}

/// Yield unwrapped_phase view of [VHFIter]::Item.
///
/// [VHFIter] releases windows where it is possible that the VHFWord's m_component requires
/// unwrapping. This function takes care of having to check if the m value has overflowed, and
/// returns the corresponding *phase* (not reduced_phase!) along with the radius value, and
/// corresponding overflow indices and overflow sign.
///
/// # Input
/// `words`: Iter view of all data originating from [VHFIter]::Item.
/// `word_offset`: This is the caller's intended 0th element, and is the n-th element starting from
/// the 0th-element of the 0th-nonempty page.
///   The rationale is that it might be possible for the caller to demand a larger view into data
///   prior to the 0th element for filtering reasons, but only has intent to write data out
///   starting from the caller's intended 0th element.
/// `get_overflow_raw`: Skip obtaining Option<...> in the return if false.
///
/// # Return
/// - `Vec<...>`:
///   - Phase: VHFWord -> Phase after accounting for need to m_overwrap.
///   - Radius: VHFWord -> Magnitude.
/// - `Option<...>`:
///   These are the private fields that should eventually populate into the private fields of
///   [WriteBlock]. Corresponding to `word_offset` argument, all MOverflowRaw from words in `words`
///   argument will not be returned!
#[inline]
fn unwrap_phases_in_window(words: [Page; WINDOW_LEN], word_offset: usize) -> PolarAndOverflowRaw {
    let mut words = {
        use std::ops::Deref;
        words.iter().flat_map(Deref::deref).copied().enumerate()
    };

    let mut phase = Vec::with_capacity(WINDOW_LEN * PAGE_LEN);
    let mut radius = Vec::with_capacity(WINDOW_LEN * PAGE_LEN);
    let mut indices = Vec::new();
    let mut signs = Vec::new();

    // Pull out the 0th element from the iterator.
    let Some(zeroth) = words.next() else {
        return PolarAndOverflowRaw::default();
    };
    {
        let Polar {
            radius: r,
            phase: p,
        } = (&zeroth.1).into();
        phase.push(p);
        radius.push(r);
    };

    // Now have to use the zeroth element and the rest of `words` to populate all the values.
    use itertools::Itertools;
    [zeroth].into_iter().chain(words).tuple_windows().fold(
        0i32,
        |mut m_offset, ((_, wa), (elem_idx, wb))| {
            // First check if there's a need to change the m_offset
            let IQMTriplet(_, _, ma) = wa.into();
            let IQMTriplet(_, _, mb) = wb.into();
            if ma.abs_diff(mb) >= M_OVERFLOW {
                match mb.cmp(&ma) {
                    Ordering::Less => {
                        m_offset += 1;
                        if elem_idx >= word_offset {
                            indices.push(elem_idx.saturating_sub(word_offset));
                            signs.push(1);
                        }
                    }
                    Ordering::Greater => {
                        m_offset -= 1;
                        if elem_idx >= word_offset {
                            indices.push(elem_idx.saturating_sub(word_offset));
                            signs.push(-1);
                        }
                    }
                    Ordering::Equal => unsafe { unreachable_unchecked() },
                }
            };
            let Polar {
                radius: r,
                phase: p,
            } = wb.into();
            phase.push(p.mul_add(m_offset as _, std::f64::consts::TAU * ((1 << 16) as f64)));
            radius.push(r);

            m_offset
        },
    );

    PolarAndOverflowRaw {
        unwrapped_phase: phase,
        radius,
        overflow_raw: if indices.is_empty() {
            None
        } else {
            Some((indices, signs))
        },
    }
}

/// Packs (radius, unwrapped phase) back into VHFWord for writing.
///
/// This is used by filters which act on unwrapped phase (with possible radius being left
/// untouched), who then have to re-pack the filtered output for [crate::runner::writer].
///
/// # Input
/// - `radius`: This is the values that were obtained prior to the filtering, which are decimated
///   as necessary to represent the phase.
/// - `phase`: This is the phase values that were obtained as a result of filtering.
/// - `idx`: The 0th element from `phase` has the corresponding `index` in the result of the
///   filtered stream.
///
/// # Assumptions
/// Assumes that both phase and idx have the same number of elements.
///
/// # Returns
/// WriteBlock as returned by [super::func::VHFItemFn].
fn pack_into_write_block(
    mut radius: impl Iterator<Item = Radius>,
    phase: impl Iterator<Item = Phase>,
    idx: usize,
) -> WriteBlock {
    let mut m_idx = Vec::new();
    let mut m_sign = Vec::new();
    let mut result = Vec::with_capacity({
        let r = radius.size_hint();
        let p = phase.size_hint();
        r.0.max(r.1.unwrap_or(0)).max(p.0).max(p.1.unwrap_or(0))
    });

    let mut phase = phase.enumerate(); // We only need to enumerate on a single of the two iterators.

    // Perform a "peek" to determine the initial state for the scan later. We cannot use a generic
    // initial element in scan because it could be misinterpreted as an overflow.
    let Some(ip0) = phase.next() else {
        assert!(
            radius.next().is_none(),
            "phase was found to be empty while radius was not"
        );
        return WriteBlock::default();
    };
    let r0 = radius
        .next()
        .expect("Expected to find radius since phase was empty");
    let word0: VHFWord = Polar {
        radius: r0,
        phase: ip0.1,
    }
    .into();
    // Peeked value has to be packed into VHFWord.
    result.push(word0);

    result.extend(phase.zip(radius).scan(
        word0,
        |prev_word, ((elem_idx, phase), radius)| -> Option<VHFWord> {
            // Obtain subsequent packed word in this scan step.
            let next_word: VHFWord = Polar { radius, phase }.into();

            // Check for m-overflow
            let IQMTriplet(_, _, ma) = (*prev_word).into();
            let IQMTriplet(_, _, mb) = next_word.into();

            // Push onto m_idx and m_sign if so.
            if ma.abs_diff(mb) >= M_OVERFLOW {
                match mb.cmp(&ma) {
                    // WARN: This currently strong limits the number of elements that can be
                    // written out to file!
                    Ordering::Less => {
                        m_idx.push(elem_idx.checked_add(idx).expect("Getting the index relative to the position with the stream is currently too large!"));
                        m_sign.push(1);
                    }
                    Ordering::Greater => {
                        m_idx.push(elem_idx.checked_add(idx).expect("Getting the index relative to the position with the stream is currently too large!"));
                        m_sign.push(-1);
                    }
                    Ordering::Equal => unsafe { unreachable_unchecked() },
                }
            };

            // Complete scan step
            *prev_word = next_word;
            Some(next_word)
        },
    ));

    let mut result = WriteBlock::new(result);
    if !m_idx.is_empty() {
        result.with_overflow(m_idx, m_sign);
    }
    result
}

/// Filtfilt but specialized for 1D-arrays with decimation and FFT-styled convolution.
///
/// # Notes
/// Default parameters of scipy's filtfilt is currently used.
///
/// Currently only supports FIR filters. See [sci_rs::signal::filter::filtfilt1_fir_fft].
///
/// # Inputs
/// - `data`: The signal where `filtfilt` is being applied to.
/// - `decimation_factor`: Every `decimation_factor`th point is taken.  
///   In more convoluted words, a `decimation_factor` of 1 does not decimate.
/// - `initial_skip`: Ignores the first `initial_skip` number of points.  
///   In other words, having a value of 0 here means the 0th data point in the result of `filtfilt`
///   will be included in the returned result.
/// - `b`: Numerator of filter.
/// - `a`: Denominator of filter.
/// - `proc`: Implementation detail - For optimizing FFT's that utilise the same convolution
///   kernel.
///
/// # Output
/// The result is an iterator to minimize the number of heap-allocations.
/// - `decimated_array`: The filtfilt resulting array is decimated in accordance to the
///   `initial_skip` and `decimation_factor`.
///
/// # Errors
/// See [sci_rs::signal::filter::filtfilt1_fir_fft].
/// As we expect this function to be ran in an unfallible context, a () as error is used.
///
/// # Assumptions
/// Number of elements in `data` (and hence the result of `filtfilt`) is greater than
/// `initial_skip`. This thus ensures that is at least one element returned from this function.
/// This way, the filtering process is not extremely lossy, as there is at least one data point per
/// input window.
/// Ideally, the lossiness of the filtering process is limited strictly to `pad`-overlapped
/// regions.
#[allow(unused_variables)]
#[inline]
fn filtfilt_f64(
    data: Array1<f64>,
    decimation_factor: NonZeroUsize,
    initial_skip: usize,
    b: ArrayView1<f64>,
    a: ArrayView1<f64>,
    proc: Arc<Mutex<impl FftProcessor<f64, f64>>>,
) -> core::result::Result<impl Iterator<Item = f64>, ()> {
    // /* WARN: Responsibility of caller to check ! */
    // assert_eq!(a.len(), 1, "filtfilt currently only supports FIR");

    let decimation_factor = decimation_factor.get();

    // Ideally this assertion should occur prior to the construction of the VHFItemFn.
    assert!(
        initial_skip < data.len(),
        "[filtfilt_f64] Decimation-related parameters is greater than number of available elements"
    );

    use sci_rs::signal::filter::{FiltFiltPad, filtfilt1_fir_fft};
    filtfilt1_fir_fft(
        b,
        data,
        Some(FiltFiltPad::default()),
        &mut *proc.lock().expect("Failed to get FFTProcessor mutex"),
    )
    .map_err(|e| {
        log::error!("[filtfilt_f64] Error occurred trying to perform filtfilt1_fir_fft!: {e}");
    })
    .map(|v| v.into_iter().skip(initial_skip).step_by(decimation_factor))
}

impl super::StreamFold {
    /// Constructs StreamFold which returns function that filters from VHFIter that allows for
    /// writing.
    ///
    /// Constructed filter follows scipy, but uses fourier convolution unlike scipy, and assumes
    /// *default* additional parameters to the filtfilt function. In principle one can think of the
    /// function as having, filtfilt applied to the data, before decimation is applied. This filter
    /// is applied on both unwrapped phase and radius before packed into writing.
    ///
    /// # Arguments
    /// - `decimation_factor`: Every `decimation_factor`th point is taken. In more convoluted
    ///   words, a `decimation_factor` of 1 does not decimate.
    /// - `filtfilt_b`: The numerator coeffecients used by
    ///   [sci_rs::signal::filter::FiltFilt::filtfilt] filter.
    /// - `filtfilt_a`: The denominator coeffecients used by
    ///   [sci_rs::signal::filter::FiltFilt::filtfilt] filter.
    ///   See panics section: Only a FIR filter is currently supported.
    /// - `filter_details`: How the values of filtfilt_b and filtfilt_a were created.
    ///   - `name`: This is the name of the filter.
    ///   - `args`: Additional parameters passed to name in the creation of the filter.
    ///   - `value`: This is the numerical values that used to perform the filters. They should be
    ///     identical to values used in `filtfilt_b` and `filtfilt_a`.
    ///
    /// # Notes
    /// For performance reasons, the filter used is in fact
    /// [sci_rs::signal::filter::filtfilt1_fir_fft].
    ///
    /// # Panics
    /// - Assumes filtfilt_b to be at least 1-element long.
    /// - filtfilt_a should currently be just `[1.0]`, as only FIR filtfilt is supported.
    // We expect this function to be called at config time, where the numerical values are being
    // interpreted from somewhere else.
    // In the case where the use demands that they wish to use Firwin to generate the values, we
    // leave it to the caller to generate the corresponding `b, a` values.
    pub fn filtfilt(
        decimation_factor: NonZeroUsize,
        filtfilt_b: &[f64],
        filtfilt_a: &[f64],
        filter_details: StreamFoldMapKernRepr<f64>,
    ) -> Self {
        // FIR specific assumptions for now.
        assert_eq!(filtfilt_a.len(), 1, "filtfilt currently only supports FIR");
        assert_eq!(
            *filtfilt_a.first().unwrap(),
            1.,
            "filtfilt currently only supports FIR"
        );
        // FIR: Normalize against a[0].
        let (b, a): (_, Array1<f64>) = {
            let a0 = *filtfilt_a.first().unwrap();
            (
                if a0 != 1. {
                    Array1::from_iter(filtfilt_b.iter().map(|&v| v / a0))
                } else {
                    Array1::from_iter(filtfilt_b.iter().cloned())
                },
                Array1::ones(1),
            )
        };
        let b_len = b.len();

        // For every window, the pages_start offset serves as a look-back into the previous window.
        // As such, we require that closure which performs filtering on VHFIter::Item to assert
        // that the assumption is upheld.
        let pages_start = decimation_factor.get().div_ceil(PAGE_LEN);
        // Clippy: !(pages_start < WINDOW_LEN)
        if pages_start >= WINDOW_LEN {
            log::error!(
                "decimation_factor (={decimation_factor}) provided into StreamFold::filtfilt is larger than what VHF_MMAP_WINDOW_LEN can support!"
            );
            assert!(
                pages_start < WINDOW_LEN,
                "Provided decimation_factor is too large for StreamFold!"
            );
        };

        let fft_processor = {
            use sci_rs::signal::filter::prelude::get_fft_processor;
            // WARN: This is likely to hinder the multiprocessed nature of the filtering step.
            // However this is necessary as it is either pariter::scope or .parallel_map_scoped
            // that cannot accept a FnMut. (See `stream.rs`.)
            Arc::new(Mutex::new(get_fft_processor()))
        };

        let func = move |(vhf_iter_idx, pages): <VHFIter as Iterator>::Item| -> WriteBlock {
            // Check assumptions of pages_start are valid
            {
                if vhf_iter_idx == 0 {
                    pages.iter().take(pages_start).for_each(|p| {
                        debug_assert!(matches!(p, Page::Empty));
                    });
                    debug_assert!(matches!(pages[pages_start], Page::Page(_)));
                } else {
                    pages.iter().take(pages_start + 1).for_each(|p| {
                        debug_assert!(matches!(p, Page::Page(_)));
                    });
                };
            };

            // Start off by phase unwrapping `pages` input.
            // We have to redo the idx_and_sign variable ourselves after filtfilt_f64.
            let PolarAndOverflowRaw {
                unwrapped_phase,
                radius,
                ..
            } = {
                // word_offset is 0, which takes in the context of the entire WINDOW for
                // m_overflow unwrapping.
                let word_offset = 0 /* TODO: Determine! */;
                unwrap_phases_in_window(pages, word_offset)
            };

            let initial_skip = 0; /* TODO: Determine! */
            // We can now decimate the phase.
            let decimated_phase = filtfilt_f64(
                Array1::from_vec(unwrapped_phase),
                decimation_factor,
                initial_skip,
                b.view(),
                a.view(),
                fft_processor.clone(),
            )
            .expect("filtfilt_f64 on unwrapped phase failed!");
            // We need to also take the correct values of phase. We average in accordance with the
            // weights provided by `b` and `a`.
            let radius = filtfilt_f64(
                Array1::from_vec(radius),
                decimation_factor,
                initial_skip,
                b.view(),
                a.view(),
                fft_processor.clone(),
            )
            .expect("filtfilt_f64 on radius failed!");

            // We take the decimated phase and regenerate the corresponding raw words and
            // idx_and_sign offset. This gives us the desired WriteBlock
            let write_idx = 0; /* TODO: Determine */
            pack_into_write_block(radius, decimated_phase, write_idx)
        };

        let func = StreamFoldFunction {
            func: Arc::new(func),
            step_by: WINDOW_LEN
                .checked_sub(pages_start)
                // Safety: Check was done at top of this function.
                .unwrap(),
            pad: pages_start,
            op: StreamFoldOp::Map(Some(MapArg {
                effective_decimation: decimation_factor,
                // NonZeroUsize::max(self, other) is nightly)
                num_before_first_drop: NonZeroUsize::new(b_len.max(decimation_factor.get()))
                    // Safety: decimation_factor is already a NonZeroUsize
                    .unwrap(),
            })),
        };

        let repr = StreamFoldRepr(Box::new([StreamFoldOpRepr::Map(Box::new(
            StreamFoldMapRepr::Filter(StreamFoldMapFilterRepr {
                filter_type: Filter::LFilter,
                filt_args: filter_details,
                step_by: decimation_factor.get(),
            }),
        ))]));

        Self { func, repr }
    }
}
