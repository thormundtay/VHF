# VHF V1 (Binary) File Specification

## Scope

* Specify the *VHF* `.bin` (V1) file structure.

## Definitions

* `Word`: 8-consecutive bytes.

## Specification

1. The first word SHALL be written in little-endian format.
2. The file MUST be an integer multiple of 8 bytes. If this condition is not
   met, it is likely a syscall writing to the file was interrupted. User
   discretion whether the file MAY be read is not outside the scope of this
   document.
3. The first word `w0` (located from the 0th byte of the file) shall conform to
   the format: `0x123456abcdef0000`, where the two least significant bytes
   (`0000`) MAY vary. The two least significant bytes SHOULD be used to
   determined the total header length `hl`, inclusive of `w0`.  
   (Number of words read: 1).
4. The next `hl - 1` words SHALL be read to complete the header. These words
   SHOULD be UTF-8 decoded to determine the condition which the file was
   written. No character in all `hl - 1` words SHALL be undecodable by UTF-8,
   i.e.: Any space after text before the end of a word MUST be zero-ed.  
   (Number of words read: `hl`)
5. If timing information is present within the header following the string `#
   recording start: `, it SHALL denote the timing information associated to the
   word located at the (8*`hl`)-th byte. If timezone information is not
   specified, it SHALL be assumed to be in local time where the recording
   occurred.
6. All data following the header SHALL consist of consecutive words. All words
   MUST be in little-endian format. Refer to the binary data specification on
   how to parse this data.
