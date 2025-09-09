//! For fold functions such as [super::StreamFold::identity_default] to describe what the function is
//! doing.

use serde::Serialize;

mod map;
pub use map::*;

/// This is the full description of its sibling: [StreamFoldFunction][super::StreamFoldFunction].
#[derive(Clone, Default, Debug, PartialEq, Serialize)]
pub struct StreamFoldRepr {
    pub repr: Box<[Representation<f64>]>,
}

/// There exists a multitude of ways in which [super::super] can do process the data. This aims to pool
/// together a collection of valid means of processing, and place the information together into a
/// single place.
#[derive(PartialEq, Eq, Clone, Debug, Serialize)]
pub enum Representation<T> {
    /// This is LFilter using a FIR. If the FIR is symmetric about some t_0, the filtering will be
    /// be a linear phase filter.
    FIRLfilter(FIR<T>),
    /// This aims to represent the relevant information of the Finite Impulse Response filter, but with
    /// time reversal double-pass, to ensure *zero-phase*, i.e.: No group-delay.
    /// This result in a filter whose transfer function is double that specified by the filter.
    FIRFiltFilt(FIR<T>),
}

/// This aims to represent the relevant information of the Finite Impulse Response filter.
#[derive(PartialEq, Eq, Clone, Debug, Serialize)]
pub struct FIR<T> {
    /// This is the decimation performed by this FIR filter.
    skip_num: usize,
    /// The convolution kernel can be derived from a well-known window, in which the name should be
    /// specified here. Otherwise, the user is free to specify their own custom window.
    #[serde(default = "Unknown")]
    filter_name: Option<String>,
    /// This is the values used in convolution.
    filter_window: Vec<T>,
}
