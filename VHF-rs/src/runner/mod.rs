use crate::{Error, Result};
use std::path::Path;

/// Checks via `fuser` if a file is in use. Returns an error if the provided path is not a file, or
/// invalid.
fn fuser_used(path: &Path) -> Result<bool> {
    let path = path.canonicalize().map_err(|_| Error::User)?;
    if path.is_dir() || !path.exists() {
        return Err(Error::User);
    }

    log::trace!("fuser on {}", path.display());

    use std::process::Command;
    Command::new("fuser")
        .arg(path.as_os_str())
        .output()
        .map_err(Error::Io)
        .map(|o| !o.stdout.is_empty())
}

/// Used in determining the running of a board, such as getting Major and Minor ID, fuser etc.
pub mod board;

/// Used in taking CLI and file configuration.
mod config;
pub use config::BoardConfig;
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
pub mod writer;
