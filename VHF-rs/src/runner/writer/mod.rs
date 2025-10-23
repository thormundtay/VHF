pub(super) mod builder;
pub use builder::WriterBuilder;
// mod netcdf_writer;
mod v1_stdout;
mod v1_writer;
mod v2_bin_writer;

pub use v1_stdout::V1StdOut;
use v1_writer::V1_MAGIC_HEADER;
pub use v1_writer::V1Writer;
pub use v2_bin_writer::V2BinWriter;
use vhf_common::data_types::{MOverflowRaw, RawVHFWord};

use crate::Result;

/// In the event that the [VHFWriter] receives less than this amount of data, no file will be
/// written. This is particularly necessary for when a trailing amount of data is created but just
/// by a tiny bit more than the user specification.
pub(super) const FILE_LAZY_LEN: usize = 1 << 13;

/// This is the data that is passed into what will eventually be written into File.
#[derive(Clone)]
pub struct WriteBlock {
    pub data: Vec<RawVHFWord>,
    /// None type is for cases where writers aren't expected to check that m_overflow_idx exists.
    m_overflow_idx: Option<Vec<usize>>,
    /// None type is for cases where writers aren't expected to check that m_overflow_idx exists.
    m_overflow_value: Option<Vec<i8>>,
}

#[allow(clippy::derivable_impls)] // The default is not guranteed to be trivial.
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
    #[allow(dead_code)]
    pub(super) fn with_overflow(&mut self, index: Vec<usize>, sign: Vec<i8>) {
        debug_assert_eq!(index.len(), sign.len());
        self.m_overflow_idx = Some(index);
        self.m_overflow_value = Some(sign);
    }

    /// For signs of m-overflows relative to start of Writeblock data, along with the indices
    /// relative to the start of write-block data, as from an iterator.
    pub(super) fn with_overflow_from_iter(
        &mut self,
        index_signs: impl Iterator<Item = MOverflowRaw>,
    ) {
        index_signs.into_iter().for_each(|MOverflowRaw(idx, val)| {
            self.m_overflow_idx
                .get_or_insert(Vec::with_capacity(512))
                .push(idx);
            self.m_overflow_value
                .get_or_insert(Vec::with_capacity(512))
                .push(val);
        });
    }

    /// Gets (idx, overflow-sign) of WriteBlock.
    pub(super) fn overflow(self) -> impl Iterator<Item = MOverflowRaw> {
        if self.m_overflow_idx.is_none() {
            Vec::new().into_iter().zip(Vec::new())
        } else {
            self.m_overflow_idx
                .unwrap()
                .into_iter()
                .zip(self.m_overflow_value.unwrap())
        }
        .map(MOverflowRaw::from)
    }
}

/// Struct which implement the following trait will consume some `&[u8;8]` to be written into the
/// file of desired type. The struct will transparently handle writing into a new file, with
/// appropriate header information.
pub trait VHFWriter {
    /// Processed or otherwise, write to the intended file the data portion.
    /// This method also silently handles dealing with any header manipulation that occurs from
    /// writing data if relevant
    fn write_data(&mut self, words: WriteBlock) -> Result<()>;

    /// Flush.
    fn close(&mut self) -> Result<()>;
}
