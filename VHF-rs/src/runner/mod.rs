/// Used in taking CLI and file configuration.
mod config;
pub use config::Configs as Config;

/// Map and Reduce are subsets of Folds.  
/// This provides all methods necessary for folding on ChunkedWindow Stream that is [VHF]'s
/// Iterator.
pub mod fold;

/// Used to run the board
mod process;
pub use process::*;

/// After the data is processed (or not) in flight, data has to be written out to somewhere. This
/// module therefore provides the means to writing into different output methods.
pub(super) mod writer;
