use super::super::types::RawVHFWord;

/// This is the data that is passed into what will eventually be written into File.
pub(super) struct WriteBlock {
    data: Vec<RawVHFWord>,
}

impl Default for WriteBlock {
    fn default() -> Self {
        WriteBlock {
            data: Vec::new(),
        }
    }
}

impl WriteBlock {
    /// Creates a new [WriteBlock] that is passed onto a
    pub(super) fn new(data: Vec<RawVHFWord>) -> Self {
        WriteBlock {
            data,
        }
    }
}
