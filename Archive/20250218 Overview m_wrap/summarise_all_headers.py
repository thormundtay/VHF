import csv
import logging
from logging import getLogger
from multiprocessing import cpu_count, Lock, Pool
from multiprocessing import current_process as current_proc
from pathlib import Path
import re
import subprocess
import sys
from typing import Iterable
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)
from VHF.parse import VHFparser


find_files_process = subprocess.check_output(["fd", "-tfile", "-E", r"'**/*{tmp,test}*'", "--extension", "bin", ".", "/mnt/nas-fibre-sensing"])
BASE_PATH = Path(__file__).parent
REGEX_LEN_FROM_STR = re.compile(r"[\._\/\-\:\_]{1}(?P<leng>\d{1,}k?m)[\._\/\-\:\_]{1}")  # ?P<name> notation is? Python specific
SEARCH_FILES = set(map(Path, find_files_process.decode().split("\n")))
SUMMARY_FILE = BASE_PATH.joinpath("summarized_headers.csv")
CSV_FIELDS = ["path", "fibre_length", "num_samples", "sparse_m_delta_length", "str_sparse_m_delta", "str_sparse_m_delta_idx"]


def init_pool_processes(the_lock):
    """Initialize each process with a global variable lock."""
    global lock
    lock = the_lock


def worker(file: Path):
    """Currently, we demand that workers can only take 1 arg for imap."""
    logger.info("worker called with args: %s", file)
    parser = None

    try:
        parser = fetch_out_parser(file)
    except Exception as exc:
        logger.critical("exc= %s", exc)
        logger.critical("", exc_info=True)
        return

    csv_write(parser)
    return


def fetch_out_parser(fname: Path) -> VHFparser:
    result = VHFparser(
        fname, headers_only=True,  # plot_duration=timedelta(seconds=0.1)
    )
    result.resolve_m_overflow_idxs()
    return result


def get_bin_files() -> set[Path]:
    """Determine which binary files left are required to be read."""
    all_files = SEARCH_FILES
    with open(SUMMARY_FILE, 'r', newline='') as file:
        paths = csv.DictReader(file)  # use default Excel dialect
        already_read: set[Path] = set(Path(x[CSV_FIELDS[0]]) for x in paths)

    return all_files - already_read


def fibre_len_from_file_name(filename: str) -> str:
    """
    From file basename, make a best effort deduction on what the length was.
    """
    result = REGEX_LEN_FROM_STR.search(filename)
    if result is None:
        return "None"
    else:
        return result.group("leng")  # This comes from REGEX_LEN_FROM_STR


def csv_write(parsed: VHFparser):
    """Takes the data pulled out from the files for specified time and writes into csv file."""
    full_path: Path = parsed._filename
    fibre_length = fibre_len_from_file_name(str(full_path.resolve()))
    num_samples = parsed._num_trc_bytes
    sparse_m_delta_length = 0 if parsed._m_mgr is None else len(parsed._m_mgr.sparse_m_delta)
    sparse_m_delta_str = None if sparse_m_delta_length == 0 else str(parsed._m_mgr.sparse_m_delta).replace("\r", "").replace("\n", "")
    sparse_m_delta_idx_str = None if sparse_m_delta_length == 0 else str(parsed._m_mgr.sparse_m_delta_idx).replace("\r", "").replace("\n", "")

    row = {
        CSV_FIELDS[0]: full_path,
        CSV_FIELDS[1]: fibre_length,
        CSV_FIELDS[2]: num_samples,
        CSV_FIELDS[3]: sparse_m_delta_length,
        CSV_FIELDS[4]: sparse_m_delta_str,
        CSV_FIELDS[5]: sparse_m_delta_idx_str,
    }

    try:
        with lock:
            logger.debug("%s acquired lock", current_proc().name)
            csv_write_core(row)
    except NameError:  # we are not in multiproc mode and have no lock
        csv_write_core(row)


def csv_write_core(row_write: dict):
    with open(SUMMARY_FILE, 'a') as file:
        dict_write = csv.DictWriter(file, fieldnames=CSV_FIELDS)
        dict_write.writerow(row_write)
        logger.info("%s Save done", current_proc().name)


def main():
    """Perform digesting of files to obtain length of sparse_m_delta."""
    files_left: Iterable[Path] = get_bin_files()
    logger.info("Running for %d files left.", len(files_left))

    csv_lock = Lock()
    nproc = cpu_count() - 1
    with Pool(processes=nproc, initializer=init_pool_processes, initargs=(csv_lock,)) as pool:
        p = pool.imap_unordered(
            worker,
            files_left
        )
        _ = list(p)  # we need something to iteratively consume up the imap to drive the pool


def main_linear():
    """Perform digesting of files down for length of sparse_m_delta without Pool."""
    logger.info("Now running for %d", len(SEARCH_FILES))
    files_left: Iterable[Path] = get_bin_files()

    for f in files_left:
        worker(f)


def create_summary_file():
    """Creates summary file"""
    if SUMMARY_FILE.exists():
        logger.info("summary file already exists. skipping...")
        return
    with open(SUMMARY_FILE, 'w+') as file:
        d_writer = csv.DictWriter(file, fieldnames=CSV_FIELDS)
        d_writer.writeheader()


if __name__ == "__main__":
    logger = getLogger()
    logger.setLevel(logging.DEBUG)
    filehandler = logging.FileHandler("summarise_all_headers_log.txt")
    filehandler.setLevel(logging.INFO)
    streamhandler = logging.StreamHandler(sys.stdout)
    streamhandler.setLevel(logging.INFO)
    fmtter = logging.Formatter(
        # the current datefmt str discards date information
        '[%(asctime)s.%(msecs)03d] (%(levelname)s) %(processName)s:%(threadName)s:%(name)s: \t%(message)s',
        datefmt="%Y-%m-%d %H:%M:%S"
    )
    filehandler.setFormatter(fmtter)
    streamhandler.setFormatter(fmtter)
    logger.addHandler(filehandler)
    logger.addHandler(streamhandler)

    if len(SEARCH_FILES) == 0:
        logger.error("Files not found! Cannot proceed!")
        sys.exit(1)

    if not SUMMARY_FILE.exists():
        logger.info("CSV Summary File could not be found, creating.")
        create_summary_file()
        proceed: bool = True
    else:
        logger.warning("CSV Summary file found. Append?")
        proceed: bool = input("Existing CSV Summary File found... Append? [y/N] ").upper() != 'N'

    logger.info("SEARCH_FILES.len = %d", len(SEARCH_FILES))

    if proceed:
        main()
        # main_linear()
