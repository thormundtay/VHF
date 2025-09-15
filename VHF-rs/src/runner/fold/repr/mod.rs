//! For fold functions such as [super::StreamFold::identity_default] to describe what the function is
//! doing.
//!
//! The high level description is currently given by [StreamFoldOpRepr::Map], which is a small
//! wrapper of [StreamFoldMapRepr]. Populate the fields of [StreamFoldMapRepr] as necessary, which
//! will then be written into the appropriate places of the save file.

use serde::Serialize;

mod map;
pub use map::*;

/// This is the full description of its sibling: [StreamFoldFunction][super::StreamFoldFunction].
///
/// # Note
/// While the current implementation of [StreamFoldRepr] allows for the representation of
/// successive filtering, this is currently not implemented in the architecture around
/// [super::StreamFoldFunction], as there still is the difficult issue of ensuring constant sized
/// blocks of data being fed into the functions, when it is possible that the number of data being
/// returned from the filtering functions to not always be the same.
// Consider that the function StreamFoldFunction.func is intended to take in multiples of a page
// worth of data at any time; this means that there might instead need to be 3 types of functions
// in a subsequent rewrite of the function signature, being:
// 1. <VHFIter as Iterator>::Item -> impl Iterator<Item = (usize?, ([RawVHFWord; ?], Option<(usize, i8)>?))>,
// 2. <^1 as Iterator>::Item -> impl Iterator<Item = ^1::Item>,
// 3. <^1 as Iterator>::Item -> impl Iterator<Item = WriteBlock>,
// Or something along such line, along with the ability to stage the intermediate Iterators for
// subsequent StreamFoldFunction.func filtering, whilst accounting for RPIT opaque types.
#[derive(Clone, Default, Debug, PartialEq, Serialize)]
pub struct StreamFoldRepr(pub Box<[StreamFoldOpRepr<f64>]>);

/// When defining a custom [super::StreamFold], the repr field emits a json, which will be used
/// here to determine the resulting JSON output.
///
/// This currently mimics the high level idea that each processing step is either a Map or a
/// Reduce.
#[derive(Clone, Default, Debug, PartialEq, Serialize)]
pub enum StreamFoldOpRepr<T>
where
    T: num_traits::Num,
{
    #[default]
    None,
    Map(Box<StreamFoldMapRepr<T>>),
    // Reduce,
}
