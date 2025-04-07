// mod netcdf_writer;
// mod v1_writer;
// mod v2_writer;

use super::super::types::RawVHFWord;
use crate::Result;

/// This is the data that is passed into what will eventually be written into File.
pub(super) struct WriteBlock {
    data: Vec<RawVHFWord>,
    /// None type is for cases where writers aren't expected to check that m_overflow_idx exists.
    m_overflow_idx: Option<Vec<usize>>,
    /// None type is for cases where writers aren't expected to check that m_overflow_idx exists.
    m_overflow_value: Option<Vec<i8>>,
}

impl Default for WriteBlock {
    fn default() -> Self {
        WriteBlock {
            data: Vec::new(),
            m_overflow_idx: None,
            m_overflow_value: None,
        }
    }
}

impl WriteBlock {
    /// Creates a new [WriteBlock] that is passed onto a Writer.
    pub(super) fn new(data: Vec<RawVHFWord>) -> Self {
        WriteBlock {
            data,
            ..WriteBlock::default()
        }
    }

    /// Creates a new [WriteBlock] from iterator that is passed onto a Writer.
    pub(super) fn new_from_iter(data: impl Iterator<Item = RawVHFWord>) -> Self {
        WriteBlock {
            data: data.collect(),
            ..WriteBlock::default()
        }
    }

    /// For signs of m-overflows relative to start of Writeblock data, along with the indices
    /// relative to the start of write-block data.
    pub(super) fn with_overflow(&mut self, index: Vec<usize>, sign: Vec<i8>) {
        debug_assert_eq!(index.len(), sign.len());
        self.m_overflow_idx = Some(index);
        self.m_overflow_value = Some(sign);
    }

    /// For signs of m-overflows relative to start of Writeblock data, along with the indices
    /// relative to the start of write-block data, as from an iterator.
    pub(super) fn with_overflow_from_iter(
        &mut self,
        index_signs: impl Iterator<Item = (usize, i8)>,
    ) {
        index_signs.into_iter().for_each(|(idx, val)| {
            self.m_overflow_idx
                .get_or_insert(Vec::with_capacity(512))
                .push(idx);
            self.m_overflow_value
                .get_or_insert(Vec::with_capacity(512))
                .push(val);
        });
    }
}
