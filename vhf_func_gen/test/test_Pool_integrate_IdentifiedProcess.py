"""With VHFPool now having been written, we should test that it works."""
from configparser import ConfigParser, ExtendedInterpolation
from datetime import timedelta
import logging
from logging import getLogger, Logger, LogRecord
from logging.handlers import QueueHandler
import multiprocessing
from multiprocessing import Lock, Queue
from multiprocessing.connection import Pipe
from typing import Callable, Generator, Mapping, Tuple
import numpy as np
import os
from pathlib import Path
import sys
from threading import Thread
module_path = str(Path(__file__).parents[2].joinpath(
    "vhf_func_gen").joinpath("run_vhf_dep"))
if module_path not in sys.path:
    sys.path.append(module_path)
# from ..run_vhf_dep.collect import FuncGenExpt
# from ..run_vhf_dep.func_gen import SyncSingleChannelFuncGen
from collect import FuncGenExpt  # noqa
from func_gen import SyncSingleChannelFuncGen  # noqa
from VHF.multiprocess.root import IdentifiedProcess  # noqa
from VHF.multiprocess.signals import ChildSignals, cont, HUP  # noqa
from VHF.multiprocess.vhf_pool import VHFPool  # noqa


def no_matplot(record: LogRecord):
    return not record.name.startswith("matplotlib") and not record.name.startswith("PIL")


class FuncGenExptRoot():
    def __init__(self, conf_path: Path, vhf_child: Callable, vhf_fail_forward: Mapping) -> None:
        self._closed: bool = False  # Determine if close() had been called
        self._init_logger()
        self._init_multiprocess_logging()

        # Take in configuration parameters
        self.conf_path = conf_path
        self.conf = ConfigParser(interpolation=ExtendedInterpolation())
        if not self._conf_check():
            self.logger.warning("conf_path provided faulty. Exiting init.")

        # Prepare database
        self.npz_lock = Lock()
        self.npz_database_path()

        # Prepare VHFPool
        IdentifiedProcess.set_process_name("VHF")
        IdentifiedProcess.set_close_delay(timedelta(seconds=18))
        # TODO: read delay from conf
        self.vhf_target = vhf_child
        self.vhf_fail_forward = vhf_fail_forward
        self.setup_child_VHFs(self.vhf_target, self.vhf_fail_forward)

    def _init_logger(self):
        """Start a logger for this class."""
        self.logger: Logger = getLogger("FuncGenRoot")
        self.logger.setLevel(logging.DEBUG)

    def _init_multiprocess_logging(self):
        """Start multiprocess safe logging."""
        self.listening_queue: Queue = Queue()
        self.listening_logger = Thread(
            target=self.__log_listener,
            args=(self.listening_queue,)
        )
        self.listening_logger.start()

    def __log_listener(self, q: Queue):
        """Listening logger to single file for multiple processes."""
        self.logger.info("[Listener] Created!")
        while True:
            record = q.get()
            if record == HUP:
                self.logger.info("[Listener] HUP!")
                break
            logger = logging.getLogger(record.name)
            logger.handle(record)

    def close(self):
        """Cleans up associated objects to this class."""
        if self._closed == True:
            r = getLogger()  # root logger
            r.warning("[FuncGenExpt] Closed invoked more than once!")
        self._closed = True

        # Kill VHFPool
        self.vhf.close()

        # Kill listening thread
        self.listening_queue.put(HUP)
        self.listening_logger.join()

    def _conf_check(self) -> bool:
        """Checks that the config file given satisfies personal+child requirements."""
        self.logger.info("conf_path = %s", self.conf_path)
        if type(self.conf_path) != Path:
            self.conf_path = Path(self.conf_path)

        self.logger.info("conf_path.exists() = %s", self.conf_path.exists())
        if not self.conf_path.exists():
            return False

        # Read conf now
        self.logger.info("Reading from conf_path into self.conf")
        self.conf.read(self.conf_path)

        # Check for personal needs now
        # 1. Check that folder for npzpath is valid.
        self.logger.info("self.conf.get(\"Paths\", \"collated_npz_save\").parent.exists() = %s", Path(
            self.conf.get("Paths", "collated_npz_save")
        ).parent.exists())
        assert Path(
            self.conf.get("Paths", "collated_npz_save")
        ).parent.exists()

        # Check that Function Generator details are correct
        for k in ["serial_no_full", "channel", "minimum_voltage", "maximum_voltage", "power_steps"]:
            assert self.conf.has_option("Function Generator", k)

        self.num_vhf_managers = int(
            eval(self.conf.get("Multiprocess", "num_vhf_managers")))
        assert self.num_vhf_managers >= 2

        self._child_conf_check()

        return True

    def _child_conf_check(self):
        """Perform the same checks as child class."""
        return

    def npz_database_path(self) -> Path:
        """If database is not found, create the numpy file instead."""
        self.npz_path = Path(
            self.conf.get("Paths", "collated_npz_save")
        )
        # Create new database if does not exist
        if not self.npz_path.exists():
            self.logger.info("Existing npz_path was not found. Creating...")
            self.create_npz_database()
        else:
            self.logger.info("Existing npz_path was found.")
            # Ensure that configuration file has the same axis as in npz_file
        return self.npz_path

    def create_npz_database(self):
        """Hydrates x-axis of npz_database for fresh run."""
        max_v = self.conf.getfloat("Function Generator", "maximum_voltage")
        min_v = self.conf.getfloat("Function Generator", "minimum_voltage")
        max_p = max_v**2.
        min_p = min_v**2.
        num_pts = 5  # For initial testing
        # num_pts = 1+int((max_p-min_p) /
        #                 self.conf.getfloat("Function Generator", "power_steps"))
        vpp_axis = np.linspace(min_p, max_p, num=num_pts)
        size = (num_pts, self.conf.getint(
            "Function Generator", "number_samples_per_step"))
        np.savez(
            self.npz_path,
            vpp=vpp_axis,
            mean=np.zeros(size),
            std_dev=np.zeros(size),
            min=np.zeros(size),
        )

    def setup_independent_variable(self):
        """Set up Tektronix Function Generator."""
        rsc_str = self.conf.get("Function Generator", "serial_no_full")
        channel = self.conf.get("Function Generator", "channel")
        # This is an independent variable being given by another file.
        self.func_gen: SyncSingleChannelFuncGen = SyncSingleChannelFuncGen(
            rsc_str, channel
        )

    def setup_child_VHFs(self, target: Callable, fail_forward: Mapping):
        """Prepare VHF Pool"""
        self.vhf = VHFPool(
            fail_forward,
            self.num_vhf_managers,
            target,
            self.listening_queue,
            conf_path=self.conf_path,
            npz_lock=self.npz_lock
        )

    # def all_indices(self) -> Generator[Tuple[int, int], None, None]:
    #     """Iterator object to yield from for passing to IdentifiedProcess[target]."""
    #     # init: find first 0 in npz
    #     max_i =  # TODO
    #     max_j =  # TODO

    #     i_start, j_start = first_zero()

    #     # yield relevant quantity
    #     for i in range(i_start, max_i+1):
    #         # prepared independent variable
    #         voltage_to_set_amplitude
    #         self.func_gen.set_amplitude(ampl_Vpp=)

    #         j_iter_start = j_start if i == i_start else 0
    #         for j in range(j_iter_start, max_j+1):
    #             # override_attr_arg is the mapping p for changing runner l, v pairs for filename
    #             # analyse_parse_arg is any *args that is taken up by *args of analyse_parse arguments
    #             # yield override_attr_arg, analyse_parse_arg
    #             yield i, j

    #     return


def first_zero_by_idx(arr: np.ndarray) -> Tuple[int, int]:
    """Yield first(i, j) in 2D numpy array."""
    unraveled_index_claim = np.argmax(arr == 0)
    if unraveled_index_claim == 0 and arr[0, 0] != 0:
        return (None, None)
    return tuple(divmod(unraveled_index_claim, arr.shape[0]))


def main():
    logger = logging.getLogger()
    logger.setLevel(logging.DEBUG)
    streamhandler = logging.StreamHandler(sys.stdout)
    streamhandler.setLevel(logging.DEBUG)
    fmtter = logging.Formatter(
        # the current datefmt str discards date information
        '[%(asctime)s.%(msecs)03d] (%(levelname)s)\t[%(processName)s:%(threadName)s] %(name)s: \t %(message)s',
        datefmt="%Y-%m-%d %H:%M:%S"
    )
    # fmtter.default_msec_format = "%s.%03d"
    streamhandler.setFormatter(fmtter)
    streamhandler.addFilter(no_matplot)
    logger.addHandler(streamhandler)

    # variable necessary to init vhf_child
    c_sig = ChildSignals()
    fail_forward = {
        c_sig.action_generic_error: True,
        c_sig.too_many_attempts: False,
    }

    # Create root process
    expt = FuncGenExptRoot(
        conf_path=Path(__file__).parents[1].joinpath(
            "VHF_FuncGen_params.ini"),
        vhf_child=FuncGenExpt,
        vhf_fail_forward=fail_forward,
    )
    # # Rewrite Vpp in npz_database_path
    # npz_path = expt.npz_database_path()
    from time import sleep
    try:
        sleep(9)
        logger.info("Sleeping for 9s more...")
        sleep(9)
    except KeyboardInterrupt:
        logger.warning("KeyboardInterrupt obtained!")
        expt.close()
        logger.warning("KeyboardInterrupt has finished running expt.close()")
        return

    expt.close()

    return


if __name__ == "__main__":
    main()
