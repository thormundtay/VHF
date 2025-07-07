"""
To test if multicore is viable.
"""

import os, sys
from pathlib import Path
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)

from collections import deque
import configparser
import datetime
import logging
from moment_multiprocess.vhf_output_analysis import vhf_output_analysis as analysis
import multiprocessing as mp
from multiprocessing import Process, Pipe
from multiprocessing.connection import Connection
from os import PathLike
import subprocess
import tempfile
from time import sleep
import threading
from typing import Union
from VHF.parse import VHF_v1_parser as VHFparser
from VHF.runner import VHFRunner

_PATH = Union[str, bytes, PathLike, Path]

def nas_thread(id:int, write_target:_PATH, tmpfile_name: _PATH, filesize):
    logging.info("[NAS %s] Spawned.", id)
    Path(write_target).parent.mkdir(parents=True, exist_ok=True)
    # print(f"{subprocess.run(['ls -la /dev/shm'], shell=True)}")
    try:
        with open(write_target, 'wb') as target, open(tmpfile_name, 'rb') as f:
            f.seek(0)
            target.write(f.read(filesize))
        logging.debug("[NAS %s] File written successfully.", id)
    except FileNotFoundError:
        logging.warning("[NAS %s] Temp file was deleted.", id)
    logging.info("[NAS %s] Thread terminating.", id)


def vhf_proc(comm: Connection, id: int, vhf_config_path: _PATH):
    """
    Process for controlling VHF board.

    This process is in charge of running the VHF board, taking its output, and
    saving and analysing the data.

    Inputs
    ------
    comm: Connection
        Pipe from parent process to receive data from.
    id: int
        Logging identifier.
    vhf_config_path: PathLike
        Location by which VHF_config properties are read from
    tol: float
        For use in analysis
    """
    logging.info("VHF process %s entered", id)

    vhf_runner = VHFRunner(vhf_config_path)
    # avoid duplicate prints
    if id == 1:
        vhf_runner.inform_params()
    conf = configparser.ConfigParser(
            interpolation=configparser.ExtendedInterpolation())
    conf.read(vhf_config_path)
    save_dir = Path(conf.get('Paths', 'save_dir'))
    npz_dir = Path(conf.get('Paths', 'npz_dir'))
    tol = conf.getfloat('Shot Noise', 'Phase Tol')

    logging.info("VHF process %s awaiting for instruction.", id)
    # Signal Ready
    comm.send_bytes(b'0')
    while data := comm.recv():
        logging.debug("[VHF %s] received %s", id, data)
        action, direction, sam_id, current = data
        direction = direction.decode()  # "Ascending/Descending text used for filename"
        sam_id = int(sam_id.decode())  # n-th sample iteration
        current = current.decode()  # Current current value being sampled over

        match action:
            case b'0':
                # Perform a VHF run
                print(f"[VHF {id}] Sampling now!")
                to_sample = True
                while to_sample:
                    f = tempfile.NamedTemporaryFile(dir='/dev/shm', delete=False)
                    fname = f.name

                    try:
                        start_time_sample = datetime.datetime.now()
                        retcode = subprocess.run(
                            **(sb_run:=vhf_runner.subprocess_run(f)),
                            stderr=subprocess.PIPE
                        )
                        filesize = f.tell()
                        logging.info("[VHF %s] subprocess.run with: %s", id, str(sb_run))
                        logging.info("[VHF %s] Retcode: %s", id, retcode)
                        to_sample = False
                    except KeyboardInterrupt:
                        logging.warning("[VHF %s] Keyboard Interrupt", id)
                        comm.send_bytes(b'1')  # Error
                    except subprocess.CalledProcessError as exc:
                        print(f"Process returned with error code {255-exc.returncode}")
                        print(f"{exc.stderr = }")
                        logging.critical("[VHF %s] CalledProcessError: exc = %s", id, exc)
                        logging.critical("[VHF %s] exc.returncode = %s", id, exc.returncode)
                        logging.critical("[VHF %s] exc.stderr = %s", id, exc.stderr)
                        logging.critical("[VHF %s] exc.stderr = %s", id, exc.stdout)
                        sleep(1)
                        # comm.send_bytes(b'1')  #  Error
                    except subprocess.TimeoutExpired as exc:
                        # logging.info("Subprocess ran with %s", str(sb_run))
                        print(f"[VHF {id}] Process Timed out! Will restart")
                        logging.critical("[VHF %s] TimeoutExpired with exception:", id)
                        logging.critical("%s", exc)
                        logging.critical("exc.stderr = ")
                        logging.critical("%s", exc.stderr)
                        # comm.send_bytes(b'1')  # Error
                    else:
                        # No error
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
                            print(f"[VHF {id}] parsing file had encountered an error!")
                            logging.warning("[VHF %s] Opened file had issues. Restarting sample...", id)
                            f.close()
                            try:
                                os.unlink(fname)
                            except FileNotFoundError:
                                None
                            sleep(1)
                            continue
                        write_nas.start()
                        comm.send_bytes(b'0')  # Signal success

                        print(f"[VHF {id}] {datetime.datetime.now().strftime('%H:%M:%S')} write_nas started")
                        logging.info("[VHF %s] File is being saved to %s", id, write_target)

                        print(f"[VHF {id}] Analysing...")
                        analysis(
                            parsedVHF,
                            # npz_dir.joinpath(f"{direction}.npz"),
                            None,
                            tol)
                        logging.info("[VHF %s] Analysis completed!", id)

                        # Terminate into next iteration of while loop only when NAS write completes
                        write_nas.join()
                        # Signal for requeue
                        comm.send_bytes(b'0')
                        print(f"[VHF {id}] Write To NAS has concluded, and thread is"
                            " ready for next action.")
                    finally:
                        # Always execute
                        f.close()
                        try:
                            os.unlink(fname)
                        except FileNotFoundError:
                            pass
                        print(f"[VHF {id}] {datetime.datetime.now().strftime('%H:%M:%S')} f.close()")

            case b'2':
                print(f"[VHF {id}] Exiting...")
                logging.info("[VHF %s] Exiting", id)
                return

            case _:
                print(f"[VHF {id}] Unrecognised message received!")
                comm.send_bytes(b'1')
                return

    print(f"[VHF {id}] Unexpected exit point! While loop exited!")
    comm.send_bytes(b'1')
    return

def main():
    """
    This is a test driver loop.

    This test aims to create a few multicore VHF controllers to see if
    staggered sampling is possible.
    """
    logging.basicConfig(filename= f"{Path(__file__).parents[2].joinpath('Log').joinpath('test-rushed_'+datetime.datetime.now().strftime('%Y%m%d_%H%M%S')+'.log')}",
        filemode='a', format='[%(asctime)s%(msecs)d] (%(levelname)s) %(name)s - \t %(message)s',
        datefmt='%H:%M:%S:',
        level=logging.DEBUG
        )

    # Create all child processes
    num_vhf_procs = 2
    vhf_pipes = [Pipe() for _ in range(num_vhf_procs)]
    vhfs = [
        Process(
            target=vhf_proc,
            args=(vhf_pipes[i][1], i, Path(__file__).parent.joinpath('vhf-params.ini'))
        ) for i in range(num_vhf_procs)
    ]
    # Start child processes
    _ = [j.start() for j in vhfs]
    # Ensure initialisation complete
    for pipe, _ in vhf_pipes:
        assert pipe.recv_bytes() == b'0', f"Error on VHF process: {pipe = }"

    # Queue of available pipes
    available_pipes = deque([p for p, _ in vhf_pipes], maxlen=len(vhf_pipes))
    # print(f"[Debug] {id(available_pipes[0]) = }")
    # print(f"[Debug] {id(vhf_pipes[0][0]) = }")
    # print(f"[Debug] {available_pipes = }")
    # print(f"[Debug] {vhf_pipes = }")

    def requeue_all_free():
        """Used for collecting all free VHF processes back into use."""
        for pipe, _ in vhf_pipes:
            # print(f"[Driver] requeue looping: {pipe = }")
            if pipe.poll():
                print(f"[Driver] Polled pipe.")
                msg = pipe.recv_bytes()
                if msg != b'0':
                    logging.warning("[Driver] Recieved issue on pipe.")

                available_pipes.append(pipe)
                print(f"[Driver] Requeued {pipe = }")
        return

    def populate_queue(s: int):
        """Ensures at least s pipes available."""
        requeue_all_free()
        while len(available_pipes) < s:
            print(f"[Driver] {len(available_pipes) = } < {s = }")
            requeue_all_free()
            sleep(1)

    # Driving loop
    runs_to_test = 8
    for i in range(runs_to_test):
        populate_queue(1)
        pipe = available_pipes.popleft()
        print(f"[Driver] Current pipe = {pipe}")
        sleep(0.5)
        pipe.send((b"0", b"Ascending", str(i).encode(), b"0.3500"))
        print(f"[Driver] Signaled to a VHF process to start.")

        sleep(12)  # hard coded to fail
        # comment out assert to fail thread
        assert pipe.recv_bytes() == b'0', f"Error obtained on VHF {i%num_vhf_procs}"

    print("[Driver] Loop is completed!")

    # Close processes
    populate_queue(len(vhf_pipes))
    _ = [p.send((b'2', b'0', b'0', b'0')) for p, _ in vhf_pipes]
    _ = [j.join() for j in vhfs]
    # Close pipes
    _ = [(p.close(), q.close()) for p, q in vhf_pipes]

    logging.debug("Main is exiting")
    print("Exiting...")
    return

if __name__ == "__main__":
    main()
