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
5. The header shall specify:
  a. The filters used
6. As m_offset is given by the header, software processing between data taken
   out of the FPGA and file writing MUST keep track of m_overflow, such that
   continuous files can have their first element be associated to the correct
   m_overflow element.
