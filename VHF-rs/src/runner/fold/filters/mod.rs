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
//!
//! [fold operations]: https://en.wikipedia.org/wiki/Fold_(higher-order_function)
