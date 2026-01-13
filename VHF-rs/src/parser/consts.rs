// XXX: This file is shared by both VHF and VHF-parse package.

/// With reference to [IQMTriplet][vhf_common::data_types::IQMTriplet]'s `M', this constant
/// determines when an overflow check should be performed.
pub const M_OVERFLOW: u16 = 0xF000;
