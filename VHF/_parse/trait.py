from abc import ABC, abstractmethod
from datetime import datetime, timedelta
import io
import os
import numpy as np
from numpy.typing import NDArray
from typing import NotRequired, TypedDict, Optional
from .v1_binary_core import BinaryVHFTrace

__all__ = [
    "_PlotTimingArg",
    "VHFparser_trait"
]


class _PlotTimingArg(TypedDict):
    start: NotRequired[datetime | timedelta]
    duration: NotRequired[timedelta]
    end: NotRequired[datetime]


class VHFparser_trait(ABC):
    @abstractmethod
    def __init__(
        self, filename: str | os.PathLike | io.BufferedRandom,
        *,
        headers_only: bool = False,
        plot_start_time: Optional[datetime | timedelta] = None,
        plot_duration: Optional[timedelta] = None,
        plot_end_time: Optional[datetime] = None,
    ) -> None:
        """Take a VHF output file and populates relevant properties."""
        ...

    @abstractmethod
    def resolve_m_overflow_idxs(self) -> None:
        """Update the class to be aware of all m-overflow indices."""
        ...

    @abstractmethod
    def update_plot_timing(self, lazy=False, **kwargs: _PlotTimingArg) -> None:
        """Change the view window associated to currently parsed file.

        lazy: bool
            Defer the fetch of the changed underlying plot window's view of the
            Trace binary data in this function call. Defaults to False.
        Refer to TraceTimer.update_plot_timing for details.
        """
        ...

    @property
    @abstractmethod
    def data(self) -> NDArray[BinaryVHFTrace.raw_word_type]:
        """Block of binary trace in accordance with plot window specified."""
        ...

    @property
    @abstractmethod
    def i_arr(self) -> NDArray[BinaryVHFTrace.i_arr_type]:
        "Block of I values in accordance with plot window specified."
        ...

    @property
    @abstractmethod
    def q_arr(self) -> NDArray[BinaryVHFTrace.q_arr_type]:
        "Block of Q values in accordance with plot window specified."
        ...

    @property
    @abstractmethod
    def m_arr(self) -> NDArray[BinaryVHFTrace.m_arr_type]:
        "Block of M values in accordance with plot window specified."
        ...

    # Properties derived from I, Q, M arrays
    @property
    @abstractmethod
    def reduced_phase(self) -> NDArray[np.float64]:
        """Block of unwrapped phase/2pi in accordance with plot window
        specified."""
        ...

    @property
    @abstractmethod
    def radii(self) -> NDArray[np.float64]:
        """Block of radius(t) in accordance with plot window specified."""
        ...
