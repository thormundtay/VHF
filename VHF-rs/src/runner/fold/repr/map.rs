//! Structs related to representing [map operations][super::super::StreamFoldOp::Map].

use serde::Serialize;
use serde::ser::SerializeMap;

/// There are a variety of Map Operations that can be done. We split them into a few
///
/// # Note
/// The variants provided are currently incomplete.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum StreamFoldMapRepr<T>
where
    T: num_traits::Num + Serialize,
{
    /// Pertaining to all filtering operations.
    Filter(StreamFoldMapFilterRepr<T>),
    /// Fourier Transform of a window of data. Often intended for product with a filtering window
    /// in frequency space.
    FourierTransform,
    /// Inverse Fourier Transform of a window of data in frequency space that is already filtered.
    /// Used to obtain data in temporal space.
    InverseFourierTransform,
}

/// Representation of [StreamFoldFunction::Map][super::super::StreamFoldFunction] filtering.
///
/// No validation is made here to ensure that this is an accurate reflaction of what the `Map`
/// function does.
// Ideal Example outputs:
// 1. Hamming under FiltFilt
// {
//   "map": "FiltFilt(Hamming)",
//   "Kernel Representation": {
//     "name": "Hamming",
//     "args": ["M=3",],
//     "value": "DiscreteFIR([0.1, 0.8,  0.1])"
//   },
//   "skip_num": 5
// }
// 2. Unknown Transfer Function (TF) under FiltFilt
// {
//   "map": "FiltFilt(Unknown)",
//   "Kernel Representation": {
//     "name": "Unknown",
//     "value": "TF([0.1, 0.8,  0.1], [1.])"
//   },
//   "skip_num": 5
// }
// 3. Butterworth in SOS representation under SOSFilt with numbers to SOS not written in example.
// {
//   "map": "SOSFilt(Butterworth)",
//   "Kernel Representation": {
//     "name": "butter",
//     "args": ["N" = 4, "Wn" = 0.125, "output" = "sos"],
//     "value": "SOS([[0.1, 0.8,  0.1], [...], [...]])"
//   },
//   "skip_num": 5
// }
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StreamFoldMapFilterRepr<T>
where
    T: num_traits::Num + Serialize,
{
    /// The filtering done in this map step.
    pub filter_type: Filter,
    /// The arguments used to obtain the filter.
    pub filt_args: StreamFoldMapKernRepr<T>,
    /// The number of elements skipped between each Map.
    pub step_by: usize,
}

/// Possible filter types used by [StreamFoldMapRepr][super::StreamFoldMapRepr].
///
/// Currently written with reference to SciPy.
///
/// # Note
/// Not every possible filtering method has yet been specified by this enum.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Filter {
    /// Filters data along one dimension using either a FIR or IIR Filter.
    ///
    /// # Notes
    /// - Filtering with FIR is symmetric about some t_0, the filtering will be be a linear phase filter.
    LFilter,
    /// Filters data along one dimension using cascaded second-order-sections.
    ///
    /// Similar to [LFilter][Filter::LFilter], but using [Second-order-sections][KernReprs::SOS].
    SOSFilt,
    /// Applies a digital filter forward and backward to a signal.
    ///
    /// This function applies a linear digital filter twice, once forward and once backwards. The
    /// combined filter has zero phase and a filter order twice that of the original.
    ///
    /// This generally expects arguments to be of the form `b, a`. It is recommended to normalize
    /// `a[0]` to 1.
    ///
    /// # Notes
    /// - The double pass ensures a zero-phase filter.
    /// - This is a non-causal filter with the time-reversal double pass.
    FiltFilt,
    /// A forward-backward digital filter using cases second-order sections.
    SOSFiltFilt,
    /// User defined.
    Other(String),
}

/// The "kernel"/window functions used by the filters.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamFoldMapKernRepr<T>
where
    T: num_traits::Num + Serialize,
{
    /// Name of the Kernel used.
    ///
    /// This could be a window or similar. Examples would include:
    /// - [Matlab-style IIR filters][1]: Butter, Butterord, Cheby1, ...
    /// - [Scipy windows][2]: Boxcar, Triangle, Hamming, Blackman, ....
    ///
    /// [1]: <https://docs.scipy.org/doc/scipy/reference/signal.html#matlab-style-iir-filter-design>
    /// [2]: <https://docs.scipy.org/doc/scipy/reference/generated/scipy.signal.get_window.html#scipy.signal.get_window>
    pub name: Option<String>,
    /// Arguments passed to Scipy used to generate [self.value][1].
    ///
    /// Intended to describe both args and keyworded args.
    /// `args` will be silently discarded if name was not given.
    ///
    /// [1]: #structfield.value
    pub arg: serde_json::Value,
    /// The value used by the function during the processing step.
    ///
    /// This would be the output of `name(arg)` that is then passed to
    /// [`StreamFoldMapRepr.filter_type`][1] for filtering.
    ///
    /// [1]: ./struct.StreamFoldMapRepr.html#structfield.filter_type
    pub value: Option<KernReprs<T>>,
}

impl<T> Serialize for StreamFoldMapKernRepr<T>
where
    T: num_traits::Num + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some({
            1 + if self.name.is_some() { 1 } else { 0 } + if self.value.is_some() { 1 } else { 0 }
        }))?;

        map.serialize_entry("name", self.name.as_ref().unwrap_or(&"Unknown".to_string()))?;

        if self.name.is_some() {
            map.serialize_entry("args", &self.arg)?;
        }
        if self.value.is_some() {
            map.serialize_entry("value", &self.value)?;
        }

        map.end()
    }
}

/// This is the numerical values of various representations describing how the output signal is
/// generated from the input signal, as used through a [filter][Filter].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum KernReprs<T>
where
    T: num_traits::Num + Serialize,
{
    /// For discrete finite impulse response filters, the discrete output signal `y[n]` as a
    /// function of the coefficients `b` and discrete input signal `x[n]` can be written as:
    /// ```custom,{class=language-latex}
    /// \[ y[n] = \sum_{i=0}^N b_i x[n-i]. \]
    /// ```
    /// Initial conditions `zi` are given by `lfilter_zi` or `lfiltic`.
    DiscreteFIRCoeff {
        /// Discrete coefficients.
        b: Box<[T]>,
        /// Initial conditions.
        #[serde(skip_serializing_if = "Option::is_none")]
        zi: Option<Box<[T]>>,
    },
    /// For discrete infinite impulse response filters, the discrete output signal `y[n]` as a
    /// function of the coefficients `b` and discrete input signal `x[n]` can be written as:
    /// ```custom,{class=language-latex}
    /// \[ y[n] = \sum_{i=0}^P b_i x[n-i] + \sum_{j=0}^Q a_j y[n-j]. \]
    /// ```
    /// Here,
    /// - `$P$` is the feedforward filter order.
    /// - `$Q$` is the feedback filter order.
    ///
    /// Thus, `$b$` is the numerator and `$a$` is the denominator.
    /// Note that `1.000 0.512 0.256` should be read as `1x^2 + 0.512x + 0.256`.
    DiscreteIIRCoeff { b: Box<[T]>, a: Box<[T]> },
    /// Transfer function. Otherwise also known as direct form.
    ///
    /// In contrast to the [DiscreteIIRCoeff][1], the polynomials here are not of discrete time
    /// indices, but of frequencies.  
    /// More commonly, a transfer function `$H(s)$` of a filter (for continuous time systems)
    /// with input `$X(s)$` and output `$Y(s)$`, where `$s = \sigma + j \omega$`, the transfer
    /// function `$H(s) = \frac{Y(x)}{X(s)}`. By setting `$\sigma = 0$`, the transfer function
    /// reduces Laplace transforms to Fourier transforms with real argument `$\omega$`, and can
    /// be thought of describing steady-state responses in frequency space.
    ///
    /// [1]: #variant.DiscreteIIRCoeff
    TF { b: Box<[T]>, a: Box<[T]> },
    /// Second Order Sections.
    ///
    /// As suggested by the name, cascading *sections* will recover the original filter.
    ///
    /// For example `[f64; 6] = 1.00000  -1.61803   1.00000   1.00000  -1.58430   0.95873`, one
    /// should read it as `(1 - 1.61803 z^-1 + z^2)/(1 - 1.58430 z^-1 + 0.95873 z^-2)`.
    ///
    /// # Further reading
    /// [Smith, J.O. Introduction to Digital Filters with Audio Applications][1]
    ///
    /// [1]: <https://ccrma.stanford.edu/~jos/fp/Series_Second_Order_Sections.html>
    SOS {
        /// Array of second-order filter coefficients.
        sos: Box<[T; 6]>,
        /// Initial conditions for cascaded filter delays.
        #[serde(skip_serializing_if = "Option::is_none")]
        zi: Option<Box<[T]>>,
    },
    /// Zero-pole-gain representation
    ///
    /// For example, a transfer function in `zpk` representation
    /// `$H(s) = 5 \frac{(s-2)(s-6)}{(s-1)(s-3)}$` will have
    /// `ZPK{z: [2, 6], p: [1, 3], k: 5}`.
    ZPK { z: Box<[T]>, p: Box<[T]>, k: T },
    /// State-space representation
    ///
    /// Given a multiple input multiple output system described by
    /// ```custom,{class=language-latex}
    /// \dot{\textbf{x}}(t) =
    /// \begin{bmatrix} -2 & -1 \\ 1 & 0 \end{bmatrix} \textbf{x}(t) +
    /// \begin{bmatrix} 1 \\ 0 \end{bmatrix} \textbf{u}(t) \\
    ///
    /// \textbf{y}(t) = \begin{bmatrix} 1 & 2 \end{bmatrix} \textbf{x}(t) +
    /// \begin{bmatrix} 1 \end{bmatrix} \textbf{u}(t)
    /// ```, the matrices `A`, `B`, `C`, `D` are thus
    /// ```custom,{class=language-python}
    /// >>> A = [[-2, -1], [1, 0]]
    /// >>> B = [[1], [0]]  # 2-D column vector
    /// >>> C = [[1, 2]]    # 2-D row vector
    /// >>> D = 1
    /// ```
    /// `input` specifies the appropriate index in the event there is more than 1 input.
    // Ideally we would use Array2<T> instead of Vec<<Vec<T>>, but we don't need ndarray in VHF
    // crate so far
    #[allow(non_snake_case)]
    SS {
        A: Vec<Vec<T>>,
        B: Vec<Vec<T>>,
        C: Vec<Vec<T>>,
        D: Vec<Vec<T>>,
        input: usize,
    },
}
