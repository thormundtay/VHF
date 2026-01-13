import pytest
from multiprocessing import Pool
from numpy.typing import NDArray
from pathlib import Path
from typing import Callable
from VHF._parse.v1_binary_core import BinaryVHFTrace
import logging
import numpy as np
import os
import subprocess

compiled_path = "usbhybrid-reference/convert_bin_to_text"
compiled = Path(__file__).parent.joinpath(compiled_path)
compile_src = Path(__file__).parent.joinpath(compiled_path+".c")
compiled_pwd = str(compiled.relative_to(Path(os.getcwd())))
chunk_size = 16*1024  # We pass in a range of 16x C's buffer size


def test_CompiledC_exists():
    """Checks for standard reference before doing other tests."""
    if compiled.exists():
        logging.info("Compiled bin_to_text found")
        assert True
        return
    else:
        logging.info("Compiled bin_to_text not found")
        if compile_src.exists():
            logging.info("Trying to compile from %s", compile_src)
            subprocess.run(
                ["gcc", "-g", "-O3", "-o", compiled_path, compiled_path+".c"],
                check=True
            )
            assert compiled.exists()

        else:
            logging.error(
                "Please try to get the C file from usbhybrid repo and place "
                "into test/usbhybrid-reference!"
            )
            assert False
    return


def q_from_stdout(x: str) -> list[str]:
    return x.split()[1::3]


def i_from_stdout(x: str) -> list[str]:
    return x.split()[0::3]


def m_from_stdout(x: str) -> list[str]:
    return x.split()[2::3]


def compiled_output(
    std_input, filt: Callable[[str], list[str]]
) -> NDArray[np.int32]:
    """See what convert_bin_to_text returns."""
    # logging.debug("compiled_pwd = %s", compiled_pwd)
    process = subprocess.run(
        [compiled_pwd],
        input=std_input,
        stdout=subprocess.PIPE
    )
    s_out = process.stdout.decode()
    result = np.array(filt(s_out), dtype=np.int32)
    return result


def range_to_Q_range(start, end) -> NDArray:
    """NDArray inclusive of start and end for Q."""
    assert 0 <= start <= 0xFFFFFF
    assert 0 <= end <= 0xFFFFFF
    result = np.arange(start, end+1, dtype=np.uint64)
    result.astype("<S8")
    return result


def range_to_I_range(start, end) -> NDArray:
    """NDArray inclusive of start and end for I."""
    assert 0 <= start <= 0xFFFFFF
    assert 0 <= end <= 0xFFFFFF
    steps = 0x1000000
    result = np.arange(start*steps, (end+1)*steps, step=steps, dtype=np.uint64)
    result.astype("<S8")
    return result


def range_to_M_range(start, end) -> NDArray:
    """NDArray inclusive of start and end for I."""
    assert 0 <= start <= 0xFFFF
    assert 0 <= end <= 0xFFFF
    steps = 0x1000000000000
    result = np.arange(start*steps, (end+1)*steps, step=steps, dtype=np.uint64)
    result.astype("<S8")
    return result


def q_core(j: int) -> None:
    q_under_test = range_to_Q_range(j*chunk_size, (j+1)*chunk_size)
    result = BinaryVHFTrace.read_q_arr(q_under_test.astype("<u8"))
    expected = compiled_output(q_under_test.tobytes(), q_from_stdout)
    assert np.all(result == expected), f"test_Q failed at {j =}"


def i_core(j: int) -> None:
    i_under_test = range_to_I_range(j*chunk_size, (j+1)*chunk_size)
    result = BinaryVHFTrace.read_i_arr(i_under_test.astype("<u8"))
    expected = compiled_output(i_under_test.tobytes(), i_from_stdout)
    assert np.all(result == expected), f"test_I failed at {j =}"


def m_core(j: int) -> None:
    m_under_test = range_to_M_range(j*chunk_size, (j+1)*chunk_size)
    result = BinaryVHFTrace.read_m_arr(m_under_test.astype("<u8"))
    expected = compiled_output(m_under_test.tobytes(), m_from_stdout)
    assert np.all(result == expected), f"test_M failed at {j =}"


def test_Q():
    """Check that unwrapping of bottom 24 bits are equivalent."""
    max_j = int(0xFFFFFF / chunk_size)
    with Pool() as p:
        p.map(q_core, range(max_j))


def test_I():
    """Check that unwrapping of middle 24 bits are equivalent."""
    max_j = int(0xFFFFFF / chunk_size)
    with Pool() as p:
        p.map(i_core, range(max_j))


def test_M():
    """Check that unwrapping of top 16 bits are equivalent."""
    max_j = int(0xFFFF / chunk_size)
    with Pool() as p:
        p.map(m_core, range(max_j))
