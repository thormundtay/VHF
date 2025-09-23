"""Provides VHF class that is to be managed by root process."""

from configparser import ConfigParser, ExtendedInterpolation
from matplotlib import pyplot as plt
from multiprocessing import Queue
from multiprocessing.connection import Connection
from multiprocessing.synchronize import Lock as LockType
from os import PathLike
from pathlib import Path
from shlex import quote
from typing import Union
import matplotlib
import multiprocessing
import numpy as np
import sys
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)
from VHF.runner import VHFRunner  # noqa
from VHF.multiprocess.vhf import genericVHF  # noqa
from VHF.parse import VHF_v1_parser as VHFparser  # noqa

_PATH = Union[str, PathLike, Path]


class FuncGenExpt(genericVHF):
    """Process for performing 1x sample of VHF and relevant followup."""

    def __init__(self, comm: Connection, parent_comm: Connection, q: Queue,
                 conf_path: _PATH, npz_lock: LockType, **kwargs):
        """Perform data collection and analysis as instructed by root.

        Communicated with via VHF Pool; in charge of collecting 1 round of
        data, keeping the data into external storage, communicate to root that
        the collection is done, then process the data and write into the common
        npz file, before awaiting subsequent call.

        Successful initialisation is signaled to root with (0, PID).

        Inputs
        ------
        comm: Connection
            For receiving instructions and sending instructions to root. Refer
            to main_func for message codes.
        parent_comm: Connection
            Parent's end of pipe for child process to close.
        q: Queue
            For sharing a queue handler to ensure multiprcess-safe logging.
            Passed into root logger before creating self.logger =
            logging.getLogger(__name__);
        npz_lock: Lock object
            This is a lock create from root that is shared across all VHF
            processes, to ensure that there is no race condition from writing
            to the shared npz file.
        **kwargs: Passed to genericVHF.
        """
        # This function is immediately runned when being started from
        # multiprocess. We need to send back to the main loop that
        # instantization has occured without issue, and pop into the waiting
        # loop.

        # 1. init class specific attributes
        super().__init__(comm, parent_comm, q, conf_path, kwargs=kwargs)
        self.npz_lock: LockType = npz_lock  # Acquired prior to writing to common npz
        # https://github.com/matplotlib/matplotlib/issues/20300#issuecomment-848201196
        matplotlib.use('agg')  # No GUI is being displayed. Saves memory.
        self.logger.debug("matplotlib set to agg backend")

        # 2. parse conf file
        self.conf_parse(conf_path)
        self.logger.debug("conf_parse complete.")

        self.vhf_runner = VHFRunner(conf_path)

        # 3. No issue found, drop into core loop.
        self.logger.info("Initialisation complete.")
        self.pid: int | None = multiprocessing.current_process().pid
        self.logger.debug("Obtained PID %s.", self.pid)
        if self.pid is None:
            # This really should not be occuring!
            self.logger.error("Failed to obtain a PID for child process.")
            self.comm.send_bytes(b'1')
            self.exit_code = 1
            self.close()
        try:
            self.comm.send((0, self.pid))
        except BrokenPipeError as e:
            self.logger.warning("Failed to sent initialisation!")
            self.logger.critical("exc = %s", e, exc_info=True)
            self.close()
        self.logger.debug("Sent init completion: %s", (0, self.pid))
        self.main_func()

    def conf_parse(self, conf_path: _PATH):
        """Initialise FuncGenExpt with properties from config file."""
        self.logger.debug(
            "Reading from configuration file, path: %s", conf_path)
        self.conf = ConfigParser(interpolation=ExtendedInterpolation())
        self.conf.read(conf_path)
        self.save_dir = Path(self.conf.get("Paths", "save_dir"))
        self.npz_path = Path(self.conf.get("Paths", "collated_npz_save"))
        self.FAIL_MAX = self.conf.getint("Multiprocess", "max_attempts")
        self.ignore_first = self.conf.getint(
            "Multiprocess", "parse_ignore_left")

    def analyse_parse(self, parsed: VHFparser, pname: str, *args):
        """Take parsed object and saves relevant bits into common npz_file."""
        npz_loc: tuple = args[0][0]

        self.logger.debug("npz_loc = %s", npz_loc)

        # Parsed data -> Relevant information
        radius = parsed.radii[self.ignore_first:]
        a = np.mean(radius)
        b = np.std(radius)
        c = np.min(radius)

        # Save data
        self.logger.debug("Trying to acquire lock.")
        self.npz_lock.acquire()
        self.logger.info("Acquired npz file and lock.")
        f = np.load(self.npz_path)
        f = dict(f)  # this step is impt, as NpzFile is immutable
        f["mean"][npz_loc] = a
        f["std_dev"][npz_loc] = b
        f["min"][npz_loc] = c
        remaining_keys = f.copy()
        for to_remove in ["mean", "std_dev", "min"]:
            remaining_keys.pop(to_remove, None)

        np.savez(
            self.npz_path,
            mean=f["mean"],
            std_dev=f["std_dev"],
            min=f["min"],
            **remaining_keys
        )
        self.npz_lock.release()
        self.logger.info("Written to npz file, and released lock.")

        # Save plot
        image_path = self.save_dir.joinpath(
            Path(pname).stem + ".png").resolve()
        # proper trim left has not been implemented, so import of plotting
        # is not quite suitable
        self.save_plot(quote(str(image_path)), parsed)

    def save_plot(self, fname: _PATH, parsed: VHFparser) -> None:
        """Save plot while still in memory."""
        def scatter_hist(y, ax_histy):
            binwidth = (np.max(y) - np.min(y)) / 100
            bins = np.arange(np.min(y) - binwidth,
                             np.max(y) + binwidth, binwidth)
            ax_histy.hist(y, bins=bins, orientation="horizontal")

        def plotphi(ax, o: VHFparser):
            t = np.arange(len(o.data)) / o.header["sampling freq"]
            ax.plot(t[self.ignore_first:], o.reduced_phase[self.ignore_first:],
                    color="mediumblue", linewidth=0.2, label="Phase")
            # ax.plot(-parsed.m_arr, color='red', linewidth=0.2, label='Manifold')
            ax.set_ylabel(r"$\phi_d$/2$\pi$rad", usetex=True)
            ax.set_xlabel(r"$t$/s", usetex=True)

        def plotr(r_ax, o: VHFparser):
            t = np.arange(len(o.data)) / o.header["sampling freq"]
            r_ax.plot(
                t[self.ignore_first:],
                rs := o.radii[self.ignore_first:],
                color="mediumblue",
                linewidth=0.2,
                label="Radius",
            )
            r_ax.set_ylabel(r"$r$/ADC units", usetex=True)
            r_ax.set_xlabel(r"$t$/s", usetex=True)
            r_ax_histy = r_ax.inset_axes([1.01, 0, 0.08, 1], sharey=r_ax)
            r_ax_histy.tick_params(axis="y", labelleft=False, length=0)
            scatter_hist(rs, r_ax_histy)
        # fig, [phi_ax, r_ax] = plt.subplots(nrows=2, ncols=1, sharex=True)
        # https://stackoverflow.com/a/65910539
        fig = plt.figure(num=1, clear=True)
        phi_ax = fig.add_subplot(2, 1, 1)
        r_ax = fig.add_subplot(2, 1, 2, sharex=phi_ax)
        plotphi(phi_ax, parsed)
        plotr(r_ax, parsed)
        view_const = 2.3
        fig.legend()
        fig.set_size_inches(view_const * 0.85 *
                            (8.25 - 0.875 * 2), view_const * 2.5)
        fig.tight_layout()
        fig.savefig(fname, dpi=300, format="png", transparent=True)
        self.logger.debug("Plot completed and saved.")
        # free memory?
        fig.clear()
        plt.close(fig)
