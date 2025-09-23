# Prototyping function to eventually merge into moment_multiprocess/vhf_thread.py

from pathlib import Path
import sys
module_path = str(Path(__file__).parents[1].joinpath("20230902 Moment Record"))
if module_path not in sys.path:
    sys.path.append(module_path)
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)

import configparser
import datetime
import logging
from moment_multiprocess.vhf_output_analysis import vhf_output_analysis as analysis
import multiprocessing as mp
from multiprocessing import Process, Pipe
from multiprocessing.connection import Connection
import os
from os import PathLike
from pathlib import Path
import subprocess
import tempfile
import threading
from time import sleep
from typing import Union
from VHF.parse import VHF_v1_parser as VHFparser
from VHF.runner import VHFRunner

logger = logging.getLogger("VHF")

_PATH = Union[str, bytes, PathLike, Path]


def nas_thread(id:int, write_target:_PATH, tmpfile_name: _PATH, filesize):
    logger.info("[NAS %s] Spawned.", id)

    # Ensure parent directory exists
    Path(write_target).parent.mkdir(parents=True, exist_ok=True)
    # print(f"{subprocess.run(['ls -la /dev/shm'], shell=True)}")

    try:
        with open(write_target, 'wb') as target, open(tmpfile_name, 'rb') as f:
            f.seek(0)
            target.write(f.read(filesize))
        logging.debug("[NAS %s] File written successfully.", id)
    except FileNotFoundError:
        logger.warning("[NAS %s] Temp file was deleted.", id)
    logger.info("[NAS %s] Thread terminating.", id)

def vhf_close_tmp_file(f, fname, id) -> None:
    logger.info("[VHF %s] Closing %s", id, fname)
    f.close()
    try:
        os.unlink(fname)
    except FileNotFoundError:
        None
    return

def vhf_sample_once(comm: Connection, vhf_runner: VHFRunner, conf_prop, direction, sam_id, current: str):
    """Aims to connect to VHF once and to sample. Returns if successful or not."""
    save_dir, npz_dir, tol = conf_prop
    f = tempfile.NamedTemporaryFile(dir='/dev/shm', delete=False)
    fname = f.name

    try:
        start_time_sample = datetime.datetime.now()
        retcode = subprocess.run(
            **(sb_run:=vhf_runner.subprocess_run(f)),
            stderr=subprocess.PIPE
        )
        filesize = f.tell()

        logger.info("[VHF %s] subprocess.run with: %s", id, str(sb_run))
        logger.info("[VHF %s] Retcode: %s", id, retcode)
        to_sample = False
    except KeyboardInterrupt:
        logger.warning("[VHF %s] Keyboard Interrupt", id)
        comm.send_bytes(b'1')  # Error
        sys.exit(0) # ?? Does this kill the thread?
    except subprocess.CalledProcessError as exc:
        logger.critical("[VHF %s] Process returned with error code %s", id, 255-exc.returncode)
        logger.critical("[VHF %s] CalledProcessError: exc = %s", id, exc)
        logger.critical("[VHF %s] CalledProcessError: exc.stderr = %s", id, exc.stderr)
        return False
    except subprocess.TimeoutExpired as exc:
        # logger.info("Subprocess ran with %s", str(sb_run))
        # print(f"[VHF {id}] Process Timed out! Will restart")
        logger.critical("[VHF %s] TimeoutExpired with exception:", id, exc)
        # comm.send_bytes(b'1')  # Error
        return False
    else:
        # No error occured during sampling

        write_nas = threading.Thread(
            target=nas_thread,
            args=(
                id,
                write_target:=(save_dir \
                    .joinpath(str(direction)) \
                    .joinpath(f"{str(start_time_sample)}_{current}A.bin")),
                fname,
                filesize
            )
        )

        try:
            parsedVHF = VHFparser(fname)
        except ValueError:
            logger.warning("[VHF %s] Opened file had issues. Restarting sample...", id)
            vhf_close_tmp_file(f, fname, id)
            return False
        write_nas.start()
        comm.send_bytes(b'0')  # Signal success

        logger.info("[VHF %s] (%s) write_nas started", id, datetime.datetime.now().strftime('%H:%M:%S'))
        logger.info("[VHF %s] File is being saved to %s", id, write_target)

        logger.info("[VHF %s] Analysing...", id)
        analysis(
            parsedVHF,
            # npz_dir.joinpath(f"{direction}.npz"),
            None,
            tol)
        logger.info("[VHF %s] Analysis completed!", id)

        # Terminate into next iteration of while loop only when NAS write completes
        write_nas.join()
        # Signal for requeue
        comm.send_bytes(b'0')
    finally:
        # Always execute
        vhf_close_tmp_file(f, fname, id)

    return True

def vhf_manager(comm: Connection, id: int, vhf_config_path: _PATH):
    """
    Process for controlling VHF board.

    This process is in charge of running the VHF board, taking its output, and
    saving and analysing the data.

    Inputs
    ------
    comm: Connection
        Pipe from parent process to recieve data from.
    id: int
        Logging identifier.
    vhf_config_path: PathLike
        Location by which VHF_config properties are read from
    tol: float
        For use in analysis
    """
    logger.info("VHF process %s entered", id)

    vhf_runner = VHFRunner(vhf_config_path)
    # avoid duplicate prints
    if id == 0:
        vhf_runner.inform_params()
    conf = configparser.ConfigParser(
            interpolation=configparser.ExtendedInterpolation())
    conf.read(vhf_config_path)
    save_dir = Path(conf.get('Paths', 'save_dir'))
    npz_dir = Path(conf.get('Paths', 'npz_dir'))
    tol = conf.getfloat('Shot Noise', 'Phase Tol')

    logger.info("VHF process %s awaiting for instruction.", id)
    # Signal Ready
    comm.send_bytes(b'0')
    while data := comm.recv():
        logging.debug("[VHF %s] recieved %s", id, data)
        action, direction, sam_id, current = data
        direction = direction.decode()  # "Ascending/Descending text used for filename"
        sam_id = int(sam_id.decode())  # n-th sample iteration
        current = current.decode()  # Current current value being sampled over

        match action:
            case b'0':
                # Perform a VHF run. Aim to restart as much as possible.
                logger.info("[VHF %s] Sampling now!", id)
                while not (
                    ret_val := vhf_sample_once(
                        comm,
                        vhf_runner,
                        (save_dir, npz_dir, tol),
                        direction,
                        sam_id,
                        current
                        )
                    ):
                    sleep(1)


            case b'2':
                # print(f"[VHF {id}] Exiting...")
                logger.info("[VHF %s] Exiting", id)
                return

            case _:
                logger.warning("[VHF %s] Unrecognised message recieved!", id)
                comm.send_bytes(b'1')
                return

    logger.warning("[VHF %s] Unexpected exit point! While loop exited!", id)
    comm.send_bytes(b'1')
    return
