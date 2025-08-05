use super::M_OFFSET;
use crate::consts::M_OVERFLOW;
use crate::{M, ParseError, ParseResult};
use bytemuck::try_cast_slice;
use itertools::Itertools;
use ndarray::Array1;
use ndarray::s as slice_macro;
use rayon::prelude::*;
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
        data_raw_map: &super::VHFparser,
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
            let mut raw_u8: Vec<u8> = vec![0; words * 8];
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
            Self::populate_remaining_m_overflow(data_raw_map, (&mut delta_idxs, &mut delta_signs))?;
        };

        // Terminate with end of file.
        debug_assert!(delta_idxs.last().cloned().unwrap_or_default() < data_raw_map.data_len);
        delta_idxs.push(data_raw_map.data_len);
        delta_signs.push(0);

        Ok(Self {
            initial_m_offset,
            delta_idxs,
            delta_signs,
        })
    }

    /// Updates the MOverflow yielded from the file.
    /// If the last element within the specified m_overflow region was not zeroed, it implies that
    /// there are more m_overflows than allocatable by file.
    ///
    /// Arguments:
    /// - v2_parser: Parent involved in creating RollOver, for read_data method
    /// - (idx, sign): Pass by mutable reference for [Self::new].
    fn populate_remaining_m_overflow(
        v2_parser: &super::VHFparser,
        idx_sign: (&mut Vec<usize>, &mut Vec<i8>),
    ) -> ParseResult<()> {
        let (delta_idx, delta_sign) = idx_sign;

        let last_idx = delta_idx.last().copied().unwrap_or(0);

        let num_bytes = v2_parser.data_len * 8;

        const BLOCK_BYTES: usize = 1 << 17;
        let raw_blocks: Vec<(usize, usize)> = (0..)
            .scan(last_idx * 8 + 8, |prev_end, _| {
                let start = *prev_end - 8;
                if start >= num_bytes - 8 {
                    return None;
                }
                let end = (start + BLOCK_BYTES).min(num_bytes);
                if end - start < 2 * 8 {
                    return None;
                }

                *prev_end = end;
                Some((start, end))
            })
            .collect();

        let mut result: Vec<_> = raw_blocks
            .into_par_iter()
            .flat_map_iter(|(start, end)| Self::per_block(v2_parser, start, end))
            .collect();
        result.sort_unstable_by_key(|v| v.0);

        result.into_iter().for_each(|MOverflowRaw(i, s)| {
            delta_idx.push(i);
            delta_sign.push(s)
        });

        Ok(())
    }

    /// For each Rayon thread to read one block of mmap.
    ///
    /// Arguments:
    /// - v2_parser for read_data method
    /// - [Start_byte, ..., End_byte-1], End_byte, ...
    ///   is the region in the mmap to read from.
    fn per_block(
        v2_parser: &super::VHFparser,
        start_byte: usize,
        end_byte: usize,
    ) -> impl Iterator<Item = MOverflowRaw> {
        debug_assert_eq!(start_byte % 8, 0);
        debug_assert_eq!(end_byte % 8, 0);

        let data_block = v2_parser
            .read_data(start_byte / 8, end_byte / 8)
            .expect("Failed to read from data_mmap");
        data_block
            .iter()
            .enumerate()
            .map(move |(i, &v)| -> (usize, RawVHFWord) { (i + (start_byte / 8), v.into()) })
            .tuple_windows()
            .filter_map(Self::idx_and_sign_for_filter_map)
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

    pub(super) fn fix_m_overflow(
        &self,
        m_arr: &mut Array1<M>,
        timer: &super::TraceTimer,
    ) -> ParseResult<()> {
        let start_idx = timer.plot_start;
        let end_idx = timer.plot_end;

        let rollover_start_idx = self.delta_idxs.partition_point(|&x| x < start_idx);
        let rollover_end_idx = self.delta_idxs.partition_point(|&x| x < end_idx);

        let initial_offset = self.delta_signs[..rollover_start_idx]
            .iter()
            .try_fold(self.initial_m_offset, |acc, &x| acc.checked_add(x as _))
            .ok_or(ParseError::Excess)?;

        (rollover_start_idx..rollover_end_idx)
            .map(|rollover_idx| {
                let s = self.delta_idxs[rollover_idx];
                let e = self.delta_idxs[rollover_idx + 1];

                (rollover_idx, s, e)
            })
            .fold(initial_offset, |acc, (rollover_idx, start, end)| {
                let m_left = start.saturating_sub(start_idx);
                let m_right = end_idx.min(end - start_idx);

                let delta = self.delta_signs[rollover_idx];
                let offset = acc + (delta as M);
                let mut slice = m_arr.slice_mut(slice_macro![m_left..m_right]);
                slice += M_OFFSET.checked_mul(offset).expect("Excess m-value");

                offset
            });

        Ok(())
    }
}
