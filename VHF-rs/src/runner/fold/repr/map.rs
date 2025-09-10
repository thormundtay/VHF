use serde::Serialize;

/// The "kernel"/window functions used by the filters.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamFoldMapKernRepr<T>
where
    T: num_traits::Num,
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
    ///
    /// [1]: #structfield.value
    pub arg: Box<[(Argument, Option<Argument>)]>,
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
    T: num_traits::Num,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        todo!()
    }
}

/// Public thin wrapper for values passed to Scipy Functions.
///
/// Used in [StreamFoldMapKernRepr].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Argument {
    String(String),
    Int(i64),
    Float(f64),
}

/// This is the numerical values of various representations describing how the output signal is
/// generated from the input signal, as used through a filter.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum KernReprs<T>
where
    T: num_traits::Num,
{
    /// For discrete finite impulse response filters, the discrete output signal `y[n]` as a
    /// function of the coefficients `b` and discrete input signal `x[n]` can be written as:
    /// ```custom,{class=language-latex}
    /// \[ y[n] = \sum_{i=0}^N b_i x[n-i]. \]
    /// ```
    /// Initial conditions `zi` are given by `lfilter_zi` or `lfiltic`.
    DiscreteFIRCoeff { b: Box<[T]>, zi: Option<Box<[T]>> },
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
