import numpy as np
from numpy.typing import NDArray


class BinaryVHFTrace:
    """Collection of methods for parsing binary data out of VHF trace.

    Binary trace data is a contiguous binary array that is interpreted a word
    at a time. Each word is 8 bytes, intended to be unpacked as (I, Q, M).
    """
    raw_word_type = np.uint64
    i_arr_type = np.int32
    q_arr_type = np.int32
    m_arr_type = np.int32

    bytes_per_word: int = 8
    potential_m_overflow_tolerance: int = 0x7F00
    # |m| > potential_m_overflow_tolerance => np.diff is then run
    actual_m_overflow: int = 0xF000  # trc[i+1] - trc[i] > THIS counts as overflowing
    m_offset = 0xFFFF + 1

    @staticmethod
    def read_i_arr(trace: NDArray[raw_word_type]) -> NDArray[i_arr_type]:
        """Gets the I portion of a word."""
        i_arr = np.bitwise_and(
            np.right_shift(trace, 24), 0xFFFFFF,
            dtype=np.dtype(BinaryVHFTrace.i_arr_type)
        )
        i_arr = i_arr - (i_arr >> 23) * 2**24
        return i_arr

    @staticmethod
    def read_q_arr(trace: NDArray[raw_word_type]) -> NDArray[q_arr_type]:
        """Gets the Q portion of a word."""
        q_arr = np.bitwise_and(
            trace, 0xFFFFFF,
            dtype=np.dtype(BinaryVHFTrace.q_arr_type)
        )
        q_arr = q_arr - (q_arr >> 23) * 2**24
        return q_arr

    @staticmethod
    def read_m_arr(trace: NDArray[raw_word_type]) -> NDArray[m_arr_type]:
        """Gets the M portion of a word."""
        # is it safe to lower the size of this?
        result = np.right_shift(trace, 48, dtype=np.dtype(np.int64))
        return result.astype(BinaryVHFTrace.m_arr_type)
