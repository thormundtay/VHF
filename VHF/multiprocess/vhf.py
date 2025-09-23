"""Abstract base class for root-managed VHF collection.

During an experiment, some independent variable is changed before the VHF board
is to sample data, write it into permanent storage (likely NAS), and also
analyse the data (ideally from a tmpfs, such as /dev/shm, to avoid additional
disk read/write), and send the analysed data into some congregated dataset.

Common things such as disk I/O can be provided in this generic class, but the
rest need to be implemented on a per-experiment basis.
"""

from abc import ABCMeta, abstractmethod, abstractproperty
from configparser import ConfigParser
from datetime import datetime
import logging
from logging import getLogger
from logging.handlers import QueueHandler
from multiprocessing import Queue
from multiprocessing.connection import Connection
import os
from os import PathLike
from pathlib import Path
from shlex import quote
import subprocess
from subprocess import PIPE
import sys
from tempfile import NamedTemporaryFile
from tempfile import _TemporaryFileWrapper
from time import sleep
from typing import Optional, Union
from .signals import HUP, cont, ChildSignals, Signals
from ..metatype import abstract_attribute
from ..runner import VHFRunner
from ..parse import VHF_v1_parser

__all__ = [
    "genericVHF",
]

_PATH = Union[str, Path, PathLike]


class genericVHF(metaclass=ABCMeta):
    """genericVHF ABC for managing concrete instance of VHF.

    This is the generic class with common methods that can be inherited from
    across multiple experiments. The concrete implementation should then be
    able to fetch data out the VHF board and process said data in a manner that
    does not become CPU blocking during data analysis, while having the next
    multiprocess (concrete) instance perform the data sampling.
    """

    # Definitions and constants (https://stackoverflow.com/a/75829903)
    FAIL_MAX: int

    def __init__(self, comm: Connection, parent_comm: Connection, q: Queue,
                 vhf_conf_path: _PATH, *args, **kwargs):
        """Provisions generic methods associated to VHF collection of traces.

        Inputs
        ------
        comm: multiprocessing.connection.Connection
           Pipe that allows child process here to send state of this process
           back to parent
        parent_comm: multiprocessing.connection.Connection
            Parent's end of pipe for child process to close.
        q: Queue
            For sharing a queue handler to ensure multiprcess-safe logging.
            Passed into root logger before creating self.logger =
            logging.getLogger(__name__);
        **kwargs
            Additional keywords. Currently supports:
            - request_requeue: bool = True
              Defaults to True. Set to False if you would like the root process
              instead of an async process, such as VHFPool with multithreaded
              requeue, to perform requeue management in a linear manner
              instead.
        """
        # 1. logging!
        self.init_log(q)
        # 2. same as parent closes child's end after fork, child should close
        # parent's end of Pipe after fork
        parent_comm.close()
        # class definitions
        self.exit_code: int = 0  # Exit code for sys.exit()
        self.comm: Connection = comm
        self.vhf_conf_path: _PATH = vhf_conf_path
        self.fail_count: int = 0
        self.c_sig = ChildSignals()
        self.request_requeue: bool = bool(kwargs.get(
            "request_requeue")) if "request_requeue" in kwargs else True

    def close(self):
        """Close up class that pulls data from VHF."""
        self.logger.info("Initiating Closing...")
        # Does q from parameters of __init__ require to be closed too?
        self.comm.close()
        self.logger.info("Closed child VHF process end of Pipe().")
        self.logger.info("Closed with exit_code %s", -self.exit_code)
        sys.exit(self.exit_code)

    def __exit__(self):
        self.close()

    def init_log(self, q: Queue) -> None:
        """Set up logger in a multiprocess-safe manner."""
        r = getLogger()  # assume that root logger settled by root thread as being
        qh = QueueHandler(q)
        if not r.handlers:
            r.addHandler(qh)
        self.logger = getLogger(__name__)
        self.logger.setLevel(logging.DEBUG)

    def sample_once(self, perm_target: _PATH) -> bool:
        """
        Take one copy of VHF trace data, and saves onto both NAS and tmpfs.

        Goals
        -----
        Signals to core_loop if sampling with tee succeeds with True, or if
        error was obtained during the sampling process, by returning False.
        Core loop should follow up by performing analysis of self._tmp copy.
        We leave for the core_loop to then take perform the data processing and
        cleanup.

        Inputs
        ------
        perm_targt: _PATH
            Permanent location for sampled data to be saved into. This should
            be file location on a permanent storage location.

        Outputs
        -------
        bool:
            Signals if data was collected with success and piped into NAS
        """
        # VHF subprocess is to write to STDOUT. Tee into perm_target, STDOUT of
        # tee goes into NamedTemporaryFile
        self.logger.info(
            "sample_once has been called. perm_target = %s",
            perm_target)
        self._tmp_open()
        self.logger.debug("perm_target = %s", perm_target)
        self.logger.debug("self._tmp.closed = %s", self._tmp.closed)
        self.logger.debug("self._tmp = %s", self._tmp)

        try:
            start_time_sample = datetime.now()
            with open(self._tmpName, "wb") as f:
                with subprocess.Popen(
                    **(sb_run := self.vhf_runner.subprocess_Popen()),
                    stdout=PIPE, stderr=PIPE
                ) as vhf:

                    # vhf.communicate(timeout=self.vhf_runner.sample_time()+3)
                    with subprocess.Popen(
                        ["tee", "-a", quote(str(perm_target))],
                        stdin=vhf.stdout, stdout=self._tmp
                    ) as retcode:
                        self.logger.debug("Retcode: %s", retcode)

            end_time = datetime.now()
            self.comm.send_bytes(self.c_sig.action_cont)
            self.logger.debug("subprocess.run had arguments: %s", str(sb_run))
            self.logger.debug("subprocess.run took time: %s",
                              end_time - start_time_sample)
            self.logger.info("Sampling success.")
            return True
        # except KeyboardInterrupt:
        #     We now handle this in the concrete implementation of genericVHF.
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
            self.logger.info("Subprocess ran with %s", str(sb_run))
            self.logger.critical("exc: %s", exc)
            self.logger.critical("exc.stderr = %s", exc.stderr)
            self.logger.critical("", exc_info=True)
            if isinstance(exc, subprocess.CalledProcessError):
                self.logger.critical(
                    f"Process returned with error code {exc.returncode}")
            return False
        # else:
        # No error occurred during sampling
        # Everything here and lower needs  to move to core_loop
        #     self.parse_data()
        #     self.write_to_collated()
        # finally:
        #     self._tmp_close()

    def _tmp_open(self) -> None:
        """For use by self.sample_once(). Creates a temporary file."""
        self._tmp: _TemporaryFileWrapper = NamedTemporaryFile(
            dir="/dev/shm", delete=False)
        self._tmpName: str = self._tmp.name
        self.logger.debug("Opened NamedTemporaryFile: %s", self._tmpName)

    def tmp_close(self) -> None:
        """Cleanup temporary file."""
        self._tmp.close()
        self._tmp = None  # Free variable up if something goes wrong
        self.logger.debug("Closed NamedTemporaryFile.")

        try:
            self.logger.debug("Deleting temporary file...")
            os.unlink(self._tmpName)
            self.logger.debug("Deleted temporary file.")
        except FileNotFoundError:
            self.logger.warning("Deleted File was not in tmp dir.")
        except Exception as e:
            self.logger.error("Exception encountered: %s", e)
            self.logger.error("", exc_info=True)

        self._tmpName = None

    def get_parsed(self, perm_target: str) -> Optional[VHF_v1_parser]:
        """Binary file obtained from stdout of VHF is parsed here.

        Tries to read data out from self._tmp, analyze, and then save into the
        common npz file. Reads from perm_target if self._tmp is missing. Raises
        b'1' error as stated in main_func() if neither could be found.

        Inputs
        ------
        perm_target: str
            sample_once would have taken duplicated the STDOUT from the
            VHFboard into (self.tmp, perm_target). This is the same
            perm_target.
        """
        def get_data(perm_target: str) -> Optional[VHF_v1_parser]:
            for attempt in (self._tmpName, perm_target):
                try:
                    parsed = VHF_v1_parser(attempt)
                    return parsed
                except FileNotFoundError:
                    self.logger.warning(
                        "VHFParser could not find %s. ", attempt)
                    continue
        return get_data(perm_target)

    # This requires that we use ABCMeta instead of ABC for class inheritance
    @abstract_attribute
    def vhf_runner(self) -> VHFRunner:
        """VHFRunner object."""
        raise NotImplementedError

    def main_func(self):
        """Awaits and acts on instructions from root process.

        We create the protocol where child processes receives all messages
        within tuples. data = (action, _).
        1. data = cont(p: mapping, npz_loc: tuple)
            Signals to continue sampling once.
            - p here is treated as mapping p to be passed to
              VHFRunner._overwrite_attr to obtain relevant file name for
              saving.
            - npz_loc here is passed into analyse_parse, to guide where in the
              common npz_file to be written to. Used directly as a slice, i.e.:
              npz_file["arr_name"][npz_loc] = new value
        2. data = signals.HUP
            Signals to gracefully terminate process.

        Messages sent back:
        1. ChildSignals.action_cont:
            Signals success of data collection. Do not send again for
            successful data analysis.
        2. ChildSignals.action_generic_error:
            Generic error. Possible situations:
            a. Tempfile created could not be opened, and file from NAS could
            also not be found.
            Please queue into available child processes upon receiving this,
            unless otherwise desired.
        3. ChildSignals.action_hup:
            Received SIGINT. Propagate up.
        4. ChildSignals.too_many_attempts:
            Repeated failed attempts to sample data.
            Root process is to decide if to close all sibling processes and
            terminate, or if to fail forward. This process is to be terminated
            still.
        5. (0, PID):
            Initialisation success, along with PID of child process.
            Relevant: multiprocessing.active_children()
        6. ChildSignals.action_request_requeue:
            Only if **kwargs has request_requeue = True. Emits a 2nd response
            after successful sampling that process is awaiting to be returned
            to join available queue.


        - During SIGINT, we aim to close all child processes gracefully, and
          only have the root process exit with exit_code 2.
        - In the even main_func fails to be able to collect the data after ?
          times, we propagate the error up and aim to close everything if
          forceful, otherwise only the existing VHF child process, and have the
          root process create a new child. This should be solely managed in
          root, and not for the child VHF process to handle.
        """
        self.logger.info("Now in main_func awaiting.")

        signal = Signals()
        c_sig = self.c_sig

        try:
            # This step is blocking until self.comm receives
            while (data := self.comm.recv()):
                self.logger.debug("Received %s", data)
                action = data[0]
                match action:
                    case signal.action_cont:  # yet to use signal.is_cont method
                        # Update how vhf_runner property will be like for
                        # sample_once to use.
                        msg = data[1]
                        self.vhf_runner._overwrite_attr(msg)

                        # Start sample.
                        self.logger.info("Sampling now!")
                        while not self.sample_once(
                            perm_target=(pname := quote(str(
                                self.save_dir.joinpath(self.vhf_runner.get_filename(
                                    self.vhf_runner.get_params()))
                            )))
                        ):
                            self.logger.warning("Failed to run VHF.")
                            self.fail_count += 1
                            if self.fail_count >= self.FAIL_MAX:
                                self.logger.error(
                                    "Exceeded allowable number of failed "
                                    "attempts trying to sample out of VHF "
                                    "board. Terminating...")
                                self.comm.send_bytes(c_sig.too_many_attempts)
                                self.close()
                            sleep(0.2)
                        # Now hand off for data analysis, and writing to npz
                        # file.
                        parsed = self.get_parsed(pname)
                        if parsed is None:
                            self.logger.error(
                                "File could not be obtained for analysis.")
                            # failed to obtain files!
                            self.comm.send_bytes(c_sig.action_generic_error)
                            self.tmp_close()  # Cleanup
                            continue

                        # Release tmp early.
                        self.tmp_close()

                        # Analyse and wrap up.
                        try:
                            self.analyse_parse(parsed, pname, data[2:])
                        except Exception as exc:
                            self.logger.critical("exc = %s", exc, exc_info=True)
                            self.comm.send_bytes(c_sig.action_generic_error)
                            continue
                        self.fail_count = 0
                        self.logger.info("Analysis and plot complete!")
                        if self.request_requeue:
                            self.comm.send_bytes(c_sig.action_request_requeue)

                    case signal.action_hup:
                        self.logger.info("Received closing signal.")
                        self.close()

                    case _:
                        self.logger.warning("Unknown message received!")
                        self.exit_code = 1
                        self.close()

        except KeyboardInterrupt:
            self.logger.warn(
                "Received KeyboardInterrupt Signal! Attempting to propagate shutdown gracefully.")
            self.comm.send(c_sig.action_hup)
            self.close()

    @abstractmethod
    def analyse_parse(self, parsed: VHF_v1_parser, pname: str, *args):
        """After having obtained sampled trace into VHFparser object,
        subclasses are free to utilise the VHFparser object to save into a
        common npz file or any other followup with the parsed trace data.

        Inputs
        ------
        parsed: VHFparser
            This object is provided for when sample_once in main_func succeeds.
        pname: str
            This is a shell escaped string for which the binary data was saved
            to permanently, if the generated temporary file had issues. Also
            provided for by main_func success.
        *args:
            Anything provided for by comm.recv()[2:]
        """
        # Use of args is in accordance with Liskov Substitution Principle.
        raise NotImplementedError

    @abstract_attribute
    def conf(self) -> ConfigParser:
        """ConfigParser object to be implemented by concrete implementation.

        This is preferably created in a conf_parse routine during init.
        """
        raise NotImplementedError

    @abstract_attribute
    def save_dir(self) -> Path:
        """Directory which trace files will be permanently saved to.

        This is preferably created in a conf_parse routine during init.
        """
        raise NotImplementedError
