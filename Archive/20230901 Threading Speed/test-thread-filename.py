"""Here, we test writing to a NAS through the use of a tempdir with filename and
having a child process perform the writing.

Must use start and join, dont use with due to scoping.

Case 1: Regular NamedTemporaryFile
use of f=NamedTemporaryFile(delete=None) with VHFparser(f.name) works

Case 2
tmpfs ('/dev/shm') created in RAM through the use of /etc/fstab to facilitate
tmp directory being in RAM.
use of f=NamedTemporaryFile(dir='/dev/shm', delete=None) with VHFparser(f.name) works

33.55s sampling takes 14.xxs to write after the tempfile has finished collecting the buffer.

Need to test with moment generator.


"""

import configparser
import datetime
import logging
import numpy as np
from os import PathLike
from pathlib import Path
from scipy.ndimage import uniform_filter1d as moving_average
from scipy.stats import moment
import subprocess
import tempfile
import threading
from typing import Union
from time import sleep

import os, sys
from pathlib import Path
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)
from VHF.parse import VHF_v1_parser as VHFparser
from VHF.runner import VHFRunner


_PATH = Union[str, bytes, PathLike, Path]

def write_to_nas(write_target:_PATH, tmpfile_name: _PATH, filesize):
    start_time = datetime.datetime.now()
    with open(write_target, 'wb') as target, open(tmpfile_name, 'rb') as f:
        f.seek(0)
        target.write(f.read(filesize))
    end_time = datetime.datetime.now()
    print(f"[NAS ({datetime.datetime.now().strftime('%H:%M:%S')})] Write to NAS took {end_time-start_time}\n[Child] end_time = {end_time.strftime('%H:%M:%S')}")
    # os.unlink(tmpfile_name) # since namedtempfile had delete=False

class Radius_Moments:
    """Returns standardised Moments."""

    def __init__(self, radii: np.ndarray):
        """

        Input
        -----
        radii: numpy 1D array
            the standardised 3nd to 6th moment with nth-root will then be
            computed. (Skew, Kurtosis, ...)
        """
        self.mean = np.mean(radii)
        self.std_dev = np.std(radii)
        # self.radii_prime = radii - self.mean
        self.moments = moment(radii, moment=range(3, 7)) / np.power(self.std_dev, range(3,7))


def get_rms(o: VHFparser, phase: np.ndarray):
    """Fixed sliding windows of width N as defined within this function.
    Returns appropriate time axis and RMSE for y-axis.
    """
    N = 32  # Window size
    start = N // 2
    end = N - start
    t = np.arange(len(phase)) / o.header["sampling freq"]
    t = t[start:-end+1]
    phase_subtract_mean = phase - moving_average(phase, N)
    phase_squared = np.power(phase_subtract_mean, 2)
    window = np.ones(N) / float(N)
    rms = np.sqrt(np.convolve(phase_squared, window, 'valid'))
    # print(f"[Debug] {np.size(t) = }\n[Debug] {np.size(rms) = }")
    return t, rms

def main():
    """Runs VHF board for some amount of time, and logs output."""


    vhf_config_path = Path(__file__).parent.joinpath('vhf-params.ini')
    vhf_runner = VHFRunner(vhf_config_path)
    vhf_runner.inform_params()
    conf = configparser.ConfigParser(
            interpolation=configparser.ExtendedInterpolation())
    conf.read(vhf_config_path)
    # confirmation_input = input("Are these parameter alright? [Y/N]: ")
    # if confirmation_input.strip().upper() != "Y":
    #     print("Stopping.")
    #     return


    # with tempfile.TemporaryDirectory() as td:
        # fn = os.path.join(td, 'tmp')
    f = tempfile.NamedTemporaryFile(
        dir='/dev/shm',
        delete=False
        )
    try:
        start_time_sample = datetime.datetime.now()
        retcode = subprocess.run(
            **(sb_run:=vhf_runner.subprocess_run(f))
        )
        filesize = f.tell()
        fn = f.name
        logging.info("Subprocess ran with %s", str(sb_run))
        logging.info("Retcode %s", retcode)
        print("Sampling collected.")
        print(f"NamedTempFile = {f}")
        print(f"\t{f.name = }")



    except KeyboardInterrupt:
        print("Keyboard Interrupt received!")

    except subprocess.CalledProcessError as exc:
        print(f"Process returned with error code {255-exc.returncode}")
        print(f"{exc.stderr = }")
    except subprocess.TimeoutExpired as exc:
        # logging.info("Subprocess ran with %s", str(sb_run))
        # logging.critical("TimeoutExpired with exception:")
        # logging.critical("%s", exc)
        # logging.critical("")
        # logging.critical("exc.stderr = ")
        # logging.critical("%s", exc.stderr)
        # logging.critical("")
        print(f"Process Timed out!")
    else: # When there is no error
        start_time = datetime.datetime.now()
        t1 = threading.Thread(target=write_to_nas,
            args=(write_target:=(Path(conf.get('Paths', 'save_dir')) \
                .joinpath(f"{str(start_time_sample)}_tempfile.bin")),
                fn,
                filesize
                )
            )

        t1.start()
        end_time = datetime.datetime.now()
        print(f"Written to {write_target}")
        print(f"[Parent ({datetime.datetime.now().strftime('%H:%M:%S')})] Started child {(end_time-start_time).seconds}s {(end_time-start_time).microseconds/1e3:.3f}ms.\n[Parent] {end_time = }")
        parsed = VHFparser(fn)



        # moment calculations
        phase = parsed.reduced_phase
        print(f"{len(phase) = }")
        # falling edge histogram
        _, rmse = get_rms(parsed, phase)
        print(f"{np.percentile(rmse, [50, 75, 90, 100]) = }")

        radius = Radius_Moments(np.sqrt(np.power(parsed.i_arr, 2.) + np.power(parsed.q_arr, 2.)))
        print(f"{radius.mean = }")
        print(f"{radius.moments = }")

        # Signal term of parent
        print(f"[Parent ({datetime.datetime.now().strftime('%H:%M:%S')})] Workload completed.")

        # Terminate and delete only when t1 is done
        t1.join()
    finally:
        f.close()
        print(f"[Parent ({datetime.datetime.now().strftime('%H:%M:%S')})] Tempfile closed!")
        # os.unlink(fn)

    # end_time = datetime.datetime.now()
    print(f"[Parent ({datetime.datetime.now().strftime('%H:%M:%S')})] All completed.")
    return

if __name__ == "__main__":
    main()
