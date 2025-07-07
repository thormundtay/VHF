from copy import deepcopy
from datetime import datetime, timedelta, timezone, tzinfo
from functools import cache, partial
import logging
from logging import getLogger
from multiprocessing import cpu_count, Lock, Pool
from multiprocessing import current_process as current_proc
from multiprocessing.managers import SyncManager, DictProxy
import numpy as np
from numpy.typing import NDArray
from pathlib import Path
from scipy.signal import decimate
import sys
from typing import Iterable, Optional
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)
from VHF.parse import VHFparser
from VHF.spec.mlab import cz_spectrogram_amplitude, detrend_linear
from VHF.stat.roll import Welford


START = Path("/mnt/nas-fibre-sensing/20231115_Cintech_Heterodyne_Phasemeter/")
SEARCH_FILES = list(START.glob("2024-01-19*.bin"))
SEARCH_FILES.extend(list(START.glob("2024-01-20*.bin")))
SEARCH_FILES.extend(list(START.glob("2024-01-21*.bin")))
SEARCH_FILES.extend(list(START.glob("2024-01-22*.bin")))
TIME_START = datetime.fromisoformat("20240119T00:30:00+0800")
TIME_END = datetime.fromisoformat("20240122T15:45:00+0800")
INTERVAL = timedelta(minutes=15)
FILES = SEARCH_FILES
NPY_FILE = Path("dbg_SAB_Collated_JanData.npz")
SPECGRAM_TOTAL_WINDOW = timedelta(seconds=120)  # seconds, per batch of update_plot_timing
SPECGRAM_WINDOW = 6  # seconds, width of sliding window

def init_pool_processes(the_lock):
    """Initialize each process with a global variable lock."""
    global lock
    lock = the_lock

def worker(timings, managed_list):
    """Currently, we demand that workers can only take 1 arg for imap."""
    # first argument has to be that for pool.imap...
    logger.info("worker called with managed_list of len: %s", len(managed_list))
    logger.info("worker called with args: %s", timings)
    return averaging_spectrogram(managed_list, timings)

@cache
def trace_endpoints(file) -> tuple[datetime, datetime]:
    """Get VHFparser.(start,end)_time as a tuple. Cached for good vibes."""
    p = VHFparser(file, headers_only=True)
    start_time: datetime = p.timings.trace_start
    end_time: datetime = p.timings.trace_end
    return (start_time, end_time)

def get_bin_files(time: datetime) -> list[Path]:
    """Obtain set of files to iterate over for time to time+interval.

    Files are included iff the provided timing is at least half of the desired
    interval.
    """

    def time_is_located_within_file_endpoints(time: datetime, file):
        start_time, end_time = trace_endpoints(file)
        return start_time <= time and time <= end_time

    start_side = list(filter(lambda x: time_is_located_within_file_endpoints(time, x), FILES))
    end_side = list(filter(lambda x: time_is_located_within_file_endpoints(time + INTERVAL, x), FILES))
    result: list[Path] = start_side
    result.extend(end_side)
    result: set[Path] = set(result)
    if len(result) == 0 or len(result) > 2:
        logger.warning("No file found for time=%s. {len(result)=%s}", time, len(result))
        pass
    if len(result) == 1:
        start, end = trace_endpoints(min(result))  # stupid way to access set item without pop
        # pop if overlap is less than helf
        if start <= time + INTERVAL <= end:
            if start - time > INTERVAL/2:
                result.pop
        elif start <= time <= end:
            if end - time < INTERVAL/2:
                result.pop

    return sorted(list(result))


def get_parsed(managed_list: DictProxy, name: Path):
    """Checks against the SyncManager's shared Dict as to if name has been
    first-pass parsed before."""
    if name not in managed_list.keys():
        logger.info("Not in memo for path = %s", name)
        result = VHFparser(name, headers_only=True)
        result.resolve_m_overflow_idxs()
        managed_list.update({name: result})
    else:
        logger.info("In memo for path = %s", name)
        result: VHFparser = managed_list[name]
        logger.debug("result.plot_window = %s", result.timings.plot_start)
        if result.timings.plot_start - result.timings.trace_start > timedelta(seconds=6):
            logger.warning("plot_start much larger than trace_start! trace_start = %s", result.timings.trace_start)
    result = deepcopy(result)
    return result


def averaging_spectrogram(managed_list: DictProxy, time: datetime):
    """We get the spectrogram for (time, time+INTERVAL), and save into npy file."""
    files = get_bin_files(time)
    logger.debug("Proc %s got files = %s", current_proc().name, list(map(lambda x: x.name, files)))
    if len(files) == 0:
        return

    averager = Welford()
    p_freq_old = None
    p_freq = None

    iteration_end = time + INTERVAL
    current_file = files.pop(0)
    parse = get_parsed(managed_list, current_file)
    current_file_start = parse.timings.trace_start
    current_file_end = parse.timings.trace_end
    plot_start = time
    old_plot_start = plot_start
    logger.debug("Proc %s about to start making spectrograms", current_proc().name)


    while plot_start < iteration_end and current_file is not None:
        plot_start = max(
            plot_start,
            current_file_start+timedelta(seconds=1)
        )  # clamp to at least the first second after any trace
        plot_end = plot_start + SPECGRAM_TOTAL_WINDOW
        parse.update_plot_timing(
            start=plot_start,
            end=plot_end
        )
        phase = parse.reduced_phase
        freq = parse.header["sampling freq"]

        if len(phase) >= SPECGRAM_WINDOW * freq:
            for factor in [2, 2, 5, 5]:
                phase = decimate(phase, factor, ftype="fir")
                freq /= factor
            if freq != 400:
                logger.warning("File %s had Freq was not decimated down to 400Hz!", current_file.name)
            logger.debug("Decimated phase down to len(phase) = %s", len(phase))

            # get Spectrogram details
            p_specgram, p_freq, _, _ = cz_spectrogram_amplitude(
                signal=phase,
                fn=(1.5, 50),
                samp_rate=freq,
                win_s=SPECGRAM_WINDOW,
                p_overlap=0.90,
                detrend_func=detrend_linear,
            )
            p_specgram = 20*np.log10(p_specgram)
            if p_freq_old is not None:
                if not np.array_equal(p_freq, p_freq_old):
                    logger.error("Didn't get the same frequencies!!")
                    logger.error("old = %s, new = %s", p_freq_old, p_freq)
            else:
                p_freq_old = p_freq

            # push into updater
            averager.update(data=p_specgram)

        # prepare for next window
        old_plot_start = plot_start
        # plot_start = plot_end - timedelta(seconds=SPECGRAM_WINDOW) + (1-0.9) * timedelta(seconds=SPECGRAM_WINDOW)
        # first subtraction is to move to start of specgram window
        # second addition is to move forward by one p-overlap
        plot_start = plot_end - 0.9*timedelta(seconds=SPECGRAM_WINDOW)
        logger.debug("Proc %s moving onto the next iteration of spectrogram window", current_proc().name)
        if plot_start > current_file_end:
            logger.debug("Proc %s attempting to fetch next file", current_proc().name)
            if len(files) == 0:
                current_file = None
            else:
                current_file = files.pop(0)
            if current_file is not None:
                logger.debug("Proc %s fetched next file", current_proc().name)
                parse = get_parsed(managed_list, current_file)
                current_file_start = parse.timings.trace_start
                current_file_end = parse.timings.trace_end
                p_freq_old = p_freq
                p_freq = None

    if p_freq_old is None:
        logger.error("p_freq found to be none!")
        logger.error("proc name: %s", current_proc().name)
        logger.error("file working on: %s", current_file)
        logger.error("plot_start: %s", old_plot_start)
        raise RuntimeError(f"p_freq none!!! file = {current_file = }, {plot_start = }")

    # perform read and write here
    freqs = p_freq_old
    avgSxx = np.zeros(shape=(1, 2, averager.mean.size))
    avgSxx[0, 0, :] = averager.mean # mean
    avgSxx[0, 1, :] = np.sqrt(averager.variance)  # std dev
    averaging_spectrogram_write(time, freqs, avgSxx)
    logger.debug("Proc %s moving onto next timer!", current_proc().name)
    return


def averaging_spectrogram_write(time: datetime, freqs: NDArray, avgSxx: NDArray):
    """Takes the data pulled out from the files for specified time and writes into npy file."""
    try:
        with lock:
            logger.debug("%s acquired lock", current_proc().name)
            averaging_spectrogram_write_core(time, freqs, avgSxx)
    except NameError:  # we are not in multiproc mode and have no lock
        averaging_spectrogram_write_core(time, freqs, avgSxx)


def averaging_spectrogram_write_core(time, freqs: NDArray, avgSxx: NDArray):
    with open(NPY_FILE, 'rb+') as file:
        try:
            npz_file = np.load(file, allow_pickle=True)  # savez pickles by default
            mode = 1
            logger.info("Read from npz file success")

        except EOFError:
            logger.info("EOF occurred, most probably trying to read from an empty file.")
            mode = 0

        if mode == 1:
            times_arr = npz_file["times"]
            freqs_arr = npz_file["freqs"]
            ampls_arr = npz_file["ampls"]
            if time in times_arr:
                logger.warning("Time %s found in npz file already...", time)
            logger.debug("freqs_arr.shape = %s, freqs.shape = %s", freqs_arr.shape, freqs.shape)
            logger.debug("ampls_arr.shape = %s, ampls.shape = %s", ampls_arr.shape, avgSxx.shape)
            times_arr = np.vstack((times_arr, np.array([time])))
            freqs_arr = np.vstack((freqs_arr, freqs))
            ampls_arr = np.vstack((ampls_arr, avgSxx))
        elif mode == 0:
            times_arr = np.array([time])
            freqs_arr = freqs
            ampls_arr = avgSxx

    with open(NPY_FILE, 'wb+') as file:
        np.savez(file, times=times_arr, freqs=freqs_arr, ampls=ampls_arr)
        logger.info("%s Save done", current_proc().name)


def main():
    """Perform digesting of files down into averaged spectrograms."""
    print(f"We are now running for {len(SEARCH_FILES) = }")
    timings: Iterable[datetime] = timings_left()
    np.random.shuffle(timings)

    global_cm = SyncManager()
    global_cm.start()  # there is no With block for SyncManager from what it seems
    GLOBAL_MEMO = global_cm.dict()
    partial_worker = partial(worker, managed_list=GLOBAL_MEMO)

    npz_lock = Lock()
    nproc = cpu_count() - 1
    with Pool(processes=nproc, initializer=init_pool_processes, initargs=(npz_lock,)) as pool:
        p = pool.imap_unordered(
            partial_worker,
            timings
        )
        y = list(p)  # we need something to iteratively consume up the imap to drive the pool

    global_cm.shutdown()


def main_linear():
    """Perform digesting of files down into averaged spectrograms without Pool."""
    print(f"We are now running for {len(SEARCH_FILES) = }")
    timings: Iterable[datetime] = timings_left()
    np.random.shuffle(timings)

    lock = Lock()
    global_memo = dict()
    for t in timings:
        worker(t, global_memo)


def aware_to_naive(t: datetime) -> datetime:
    """Replace tzinfo to None while keeping the hour parameter, mimicking local time.

    This is needed because np.datetime64 otherwise takes the UTC+0 equivalent of t.
    """
    offset_tz: Optional[tzinfo] = t.tzinfo
    if offset_tz is not None:
        offset = offset_tz.utcoffset(t)
    else:
        offset = timedelta(hours=0)
    result = t.astimezone(timezone(timedelta(hours=0))) + offset
    return result.replace(tzinfo=None)


def naive_to_aware(t: datetime, h:int=8) -> datetime:
    """Replace tzinfo to be aware."""
    return t.astimezone(timezone(timedelta(hours=h)))


def datetime64_to_dt(t: np.datetime64) -> datetime:
    """Takes a numpy datetime64 and give a naive python datetime."""
    return datetime.strptime(t.astype(str)[:-3], '%Y-%m-%dT%H:%M:%S.%f')


def timings_left() -> list[datetime]:
    """Array of time-axis data points still needed."""
    # np.arange will preferably generate np.datetime64 from TIME_START: datetime.datetime
    # this will consequently cast from TZ+X to TZ+0.
    timings = np.arange(aware_to_naive(TIME_START), aware_to_naive(TIME_END), INTERVAL)
    timings = map(datetime64_to_dt, timings)
    timings = map(naive_to_aware, timings)
    timings = list(timings)

    with open(NPY_FILE, 'rb+') as file:
        try:
            npz_file = np.load(file, allow_pickle=True)  # savez pickles by default
            logger.info("Read from npz file success")
            times_arr = npz_file["times"]

        except EOFError:
            logger.info("EOF occurred, most probably trying to read from an empty file.")
            times_arr = np.array([])

    times_arr = np.sort(times_arr.flatten())
    try:
        logger.debug("From file, first and last (temporal) times found to have already been written: %s, %s", min(times_arr), max(times_arr))
    except ValueError:
        logger.debug("Nothing from file")
    only_in_timings = np.setdiff1d(timings, times_arr)
    logger.info("Remaining times found needed doing: only_in_timings = %s", only_in_timings)
    return only_in_timings


def no_matplot(msg: logging.LogRecord):
    return not msg.name.startswith("matplotlib") and not msg.name.startswith("PIL")


if __name__ == "__main__":
    logger = getLogger()
    logger.setLevel(logging.DEBUG)
    logger.addFilter(no_matplot)
    streamhandler = logging.StreamHandler(sys.stdout)
    streamhandler.addFilter(no_matplot)
    streamhandler.setLevel(logging.INFO)
    fmtter = logging.Formatter(
        # the current datefmt str discards date information
        '[%(asctime)s.%(msecs)03d] (%(levelname)s) %(processName)s:%(threadName)s:%(name)s: \t%(message)s',
        datefmt="%Y-%m-%d %H:%M:%S"
    )
    streamhandler.setFormatter(fmtter)
    logger.addHandler(streamhandler)

    if not NPY_FILE.exists():
        logger.info("NPY File could not be found, creating.")
        NPY_FILE.touch()
    else:
        logger.info("NPY file found. Proceeding...")

    main()
    # main_linear()
