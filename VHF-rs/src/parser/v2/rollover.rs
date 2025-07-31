use crate::consts::M_OVERFLOW;
use crate::{M, ParseError, ParseResult};
use bytemuck::try_cast_slice;
use itertools::Itertools;
use memmap2::MmapOptions;
use std::{
    cmp::Ordering,
    fs::File,
    hint::unreachable_unchecked,
    io::{BufReader, Read},
    path::Path,
};
use vhf_common::data_types::{IQMTriplet, MOverflowRaw, RawVHFWord};
use vhf_common::write_types::MOverflowWrite;

#[derive(Debug)]
pub(super) struct RollOver {
    /// This is the offset of the 0th data point.
    initial_m_offset: M,
    /// Index where the offset changes.
    delta_idxs: Vec<usize>,
    /// Keep only the sign for where an offset occurs.
    delta_signs: Vec<i8>,
}

impl RollOver {
    /// Constructs a dense representation of all m-overflows.
    ///
    /// At init time, will read the rest of the file if the m-overflow allocated by the file was
    /// insufficient.
    ///
    /// Arguments:
    /// - file: [Path] to open
    /// - offset: Bytes to offset from file start for m-overflows.
    /// - words: Number of words (u64s) to read.
    pub(super) fn new(
        file: &Path,
        offset: usize,
        words: usize,
        initial_m_offset: M,
    ) -> ParseResult<Self> {
        if offset % 8 != 0 {
            log::error!("Header not flushed to word boundary!");
            return Err(ParseError::ValueError);
        }

        let (mut delta_idxs, mut delta_signs) = {
            // Scope inside so that File is closed prior to passing to
            // populate_remaining_m_overflow.
            let mut file_handle = BufReader::new(File::open(file)?);
            file_handle.seek_relative(offset as _)?;
            let mut raw_u8: Vec<u8> = Vec::with_capacity(words * 8);
            let raw: &[i64] = {
                if words > 0 {
                    file_handle.read_exact(&mut raw_u8)?;
                    try_cast_slice(&raw_u8)? // WARN: This assumes endianness!
                } else {
                    &[]
                }
            };

            let mut idxs = Vec::with_capacity(words);
            let mut signs = Vec::with_capacity(words);

            raw.iter()
                .take_while(|&&v| v != 0)
                .try_for_each(|&v| -> ParseResult<()> {
                    let MOverflowRaw(i, s) = MOverflowWrite(v)
                        .try_into()
                        .map_err(|_| ParseError::InternalError)?; // Only failure mode of From
                    idxs.push(i);
                    signs.push(s);

                    Ok(())
                })?;

            (idxs, signs)
        };

        if delta_signs.len() == words {
            let data_offset = offset
                .checked_add(words.checked_mul(8).ok_or(ParseError::Excess)?)
                .ok_or(ParseError::Excess)?;
            Self::populate_remaining_m_overflow(
                file,
                data_offset,
                (&mut delta_idxs, &mut delta_signs),
            )?;
        };

        Ok(Self {
            initial_m_offset,
            delta_idxs,
            delta_signs,
        })
    }

    /// Updates the MOverflow yielded from the file.
    /// If the last element within the specified m_overflow region was not zeroed, it implies that
    /// there are more m_overflows than allocatable by file.
    /// Arguments:
    /// - file: Path
    /// - data_offset: Number of bytes leading up to the first word of data.
    /// - (idx, sign):
    fn populate_remaining_m_overflow(
        file: &Path,
        data_offset: usize,
        idx_sign: (&mut Vec<usize>, &mut Vec<i8>),
    ) -> ParseResult<()> {
        let (delta_idx, delta_sign) = idx_sign;

        let last_idx = delta_idx.last().copied().unwrap_or(0);
        let offset = last_idx
            .checked_add(data_offset as _)
            .ok_or(ParseError::Excess)?;

        let mmap = unsafe {
            MmapOptions::new()
                .offset(offset as _)
                .map(&File::open(file)?)
        }?;
        let raw: &[u64] = try_cast_slice(&mmap)?; // Current mallocs the entire file into u64

        raw.iter()
            .enumerate()
            .skip(last_idx)
            .map(|(idx, &v)| -> (usize, RawVHFWord) { (idx, v.into()) })
            .tuple_windows()
            .filter_map(Self::idx_and_sign_for_filter_map)
            .for_each(|v| {
                delta_idx.push(v.0);
                delta_sign.push(v.1);
            });

        Ok(())
    }

    /// Filter on file by index to get delta_idx and delta_sign.
    ///
    /// This is not identical to identity_fold in fold/mod.rs.
    fn idx_and_sign_for_filter_map(
        ((_, a), (b_idx, b)): ((usize, RawVHFWord), (usize, RawVHFWord)),
    ) -> Option<MOverflowRaw> {
        let IQMTriplet(_, _, a) = a.into();
        let IQMTriplet(_, _, b) = b.into();
        if a.abs_diff(b) >= M_OVERFLOW {
            match b.cmp(&a) {
                // The 2nd element of the window found to be less => overflow to negative
                Ordering::Less => Some((b_idx, 1).into()),
                // The 2nd element of the window found to be more => underflow to positive
                Ordering::Greater => Some((b_idx, -1).into()),
                // Safety: M_OVERFLOW check above.
                Ordering::Equal => unsafe { unreachable_unchecked() },
            }
        } else {
            None
        }
    }
}
