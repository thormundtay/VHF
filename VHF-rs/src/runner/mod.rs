/// Used in taking CLI and file configuration.
mod config;
pub use config::Configs as Config;

/// Used to run the board
mod process;
pub use process::*;
