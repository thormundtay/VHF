"""
Perform analysis of vhf output in both radius and phase.

1. Root-mean-square error of phi(t) is studied, and the population statistics is
   determined.
2. Radius statistics (mean, std, n-th standard moments) is captured and saved.
3. Large fluctuation in phi(t), say, of at least 4000 rads within 33s, is
   recognised as multimode/faulty data.
Analysis is consequently saved into a npz file.
"""

import os, sys
from pathlib import Path
module_path = str(Path(__file__).parents[1])
if module_path not in sys.path:
    sys.path.append(module_path)
module_path = str(Path(__file__).parents[3])
if module_path not in sys.path:
    sys.path.append(module_path)

import logging
import numpy as np
from os import PathLike
from pathlib import Path
from scipy.ndimage import uniform_filter1d as moving_average
from scipy.stats import moment
from typing import Union
from VHF.parse import VHF_v1_parser as VHFparser

_PATH = Union[str, bytes, PathLike, Path]

def vhf_output_analysis(parsed: VHFparser, npz_file: _PATH, tol: float):
    """
    Inputs
    ------
    datafile: (Temporary) File as generated from VHF board
    npz_file: File to save npz data to
    tol: if (max-min)[phi(t)] > tol, denoted laser as being multi-mode
    """
    # parsed = VHFparser(datafile)
    # parsed.ignore_first(50000)

    phase = parsed.reduced_phase

    # Criterion 3:
    single_mode = True
    phase_fluc = np.max(phase) - np.min(phase)
    if phase_fluc > tol:
        single_mode = False

    # Criterion 2:
    moments = np.zeros(6)
    radii = parsed.radii
    moments[0] = np.mean(radii)
    moments[1] = np.std(radii)
    moments[2:] = moment(radii, moment=range(3, 7)) / np.power(moments[1], range(3,7))

    # Criterion 1:
    hist_data = np.percentile(phase, [50, 75, 90, 100])

    print(f"{single_mode = }")
    print(f"{moments = }")
    print(f"{hist_data = }")
    return
