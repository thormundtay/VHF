mod error;
pub use error::Error;
pub use error::Result;

/// This mod is used for reading data out from files and all relevant functions involved in the
/// process of interpreting the files for data analysis.
pub mod parser;

/// Determines things associated to the startup and running of streaming data in VHF board,
/// primarily through [runner::VHF].
pub mod runner;

/// Data types common to all VHF related operations.
pub mod types;
