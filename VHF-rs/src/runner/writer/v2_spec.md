# VHF V2 (Binary) File Specification

## Scope

* Specify the *VHF* `.vhf-bin` (V2) file structure.

## Non-goals

* This document does not aim to define the *VHF* `.vhf-hex` (V2) file
structure.

## Definitions

* `Word`: 8-consecutive bytes.

## Specification

1. The first word `VHFV2BIN` SHALL be written in UTF-8.
2. The next two bytes are byte-order marks. They MUST be read to determine if
   all binary words in the file are to be read in LE or BE.
3. The next eight bytes SHALL be UNIX native-endian time-stamps that give a
   incomplete start time of the data set.
4. The next eight bytes SHALL be native-endian u64 describing how long the
   *remaining* of the header is in bytes.
5. The header MUST be written in UTF-8. The header shall specify:
   a. The filters used, in the sequence the filters were applied.
   b. The time associated to the first data point, as given by key
      `file_start`. This is not necessarily the same as the starting time of
      the VHF board.
   c. The number of elements in the file after the header used for storing
      indices associated to m_overflow, given by key `m_overflow_total`.
6. As m_offset is given by the header, software processing between data taken
   out of the FPGA and file writing MUST keep track of m_overflow, such that
   continuous files can have their first element be associated to the correct
   m_overflow element.
7. The header is then 0-flushed up to the next word boundary, similar to v1.
8. The next `m_overflow_total` words are indices which `m_overflow` has
   occured relative to the start of the file.
   a. If the most-significant-bit (in native-endian) is 0, a +1 `m_overflow`
      has occured. Zeroing this most-significant-bit gives the index which the
      overflow has occurred on.
   b. If the most-significant-bit (in native-endian) is 1, a -1 `m_overflow`
      has occured. Zeroing this most-significant-bit gives the index which the
      overflow has occurred on.
   c. Example: An index of `3` with a `m_overflow` of +1 indicates that the
      phase located at index 3 has an m_offset that is +1 compared to the phase
      located at index 3-1.  
      Reference test: `runner::process::test_vhf_step_fold::stepped_overlapping_identity_b`.
   d. There SHALL not be any indices within the "`m_overflow` block" that is
      not present in the file.
9. If the final word in the "`m_overflow` block" is non-zero, the parser
   shall determine the remaining of the sign changes.
