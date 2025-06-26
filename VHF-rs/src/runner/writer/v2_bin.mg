# VHF Version 2 Binary File Format

# See: file(1) magic(5) for locally observed files

0	string	VHFV2BIN	VHF Version 2 Binary file
# x- denotes experimental type
!:mime	application/x-vhfv2bin

>8	beshort	0xFEFF	(Big-Endian data)
>>10	beqldate	>0	Unix Timestamp: %s
>8	leshort	0xFFFE	(Little-Endian data)
>>10	leqldate	>0	Unix Timestamp: %s
