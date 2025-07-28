import datetime
import logging
from pathlib import Path
from plot_VHF_output import plot_rad_spec
from matplotlib import pyplot as plt
import numpy as np
import subprocess
from subprocess import PIPE
import sys
from tempfile import TemporaryDirectory
from VHF.parse import VHF_v1_parser
from VHF.runner import VHFRunner


def tmp_store_file(dir: str) -> Path:
    """Get the file saved in the temp store folder."""
    all_files = [f for f in Path(dir).iterdir() if f.is_file()]
    if len(all_files) > 1:
        logging.warning("More than 1 file found in temp folder!")
    return all_files[0]


def run_and_plot():
    """Runs VHF board for some amount of time, and displays out temporarily."""

    print("\x1b[41mRuns VHF board and shows 15s of data. Does not save data!\x1B[0m")  # noqa: E501
    vhf_config_path = Path(__file__).parent.joinpath('VHF_board_params.ini')
    vhf_runner = VHFRunner(
        vhf_config_path,
        force_to_buffer=True,
        overwrite_properties={
            'skip_num': 4,
            'num_samples': 2**23,
            'v': 3,
            '-num_files': 1
        }
    )
    vhf_runner.inform_params()
    # confirmation_input = input("Are these parameter alright? [y/N]: ")
    # if confirmation_input.strip().upper() != "Y":
    #     print("Stopping.")
    #     return

    logging.basicConfig(
        filename=datetime.datetime.now().strftime(
            "Log/runandplotVHF_%Y%m%d_%H%M%s.log"
        ),
        filemode="w",
        format="[%(asctime)s] %(name)s -\t%(levelname)s -\t%(message)s",
        level=logging.DEBUG,
    )

    with TemporaryDirectory(prefix="VHF") as tmp_store:
        emsg = 0
        start_time = datetime.datetime.now()

        sb_run = vhf_runner.subprocess_run(tmp_dir=tmp_store)
        try:
            retcode = subprocess.run(**sb_run, stderr=PIPE)
            logging.info("Subprocess ran with %s", str(sb_run))
            logging.info("Retcode %s", retcode)
        except KeyboardInterrupt:
            logging.info("Subprocess ran with %s", str(sb_run))
            logging.info("Keyboard Interrupted")
            print("Keyboard Interrupt received!")
            sys.exit(0)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:  # noqa: E501
            logging.info("Subprocess ran with %s", str(sb_run))
            logging.critical("exc: %s", exc)
            logging.critical("exc.stderr = ", exc.stderr)
            logging.critical("", exc_info=True)
            if isinstance(exc, subprocess.CalledProcessError):
                print(f"Process returned with error code {exc.returncode}")
            print(f"{exc.stderr=}")
            emsg = 1

        end_time = datetime.datetime.now()
        print(f"Sampling was ran for {(end_time - start_time)}.")
        logging.info("Sampling ended at %s", end_time)
        if emsg != 0:
            sys.exit(-emsg)

        tmp_store_name = tmp_store_file(tmp_store)
        parsed = VHF_v1_parser(tmp_store_name)
        logging.debug("min(m) = %f, max(m) = %f", np.min(parsed.m_arr), np.max(parsed.m_arr))

    phase = parsed.reduced_phase
    print(f"Phase mean: {phase[12000:].mean()}\nPhase Std Dev: {phase[12000:].std()}")
    fig = plot_rad_spec(True, False, True)(parsed, phase)
    view_const = 2.3
    fig.legend()
    fig.set_size_inches(view_const * 0.85 *
                        (8.25 - 0.875 * 2), view_const * 2.5)
    fig.tight_layout()
    fig.canvas.manager.set_window_title(str(tmp_store_name))
    plt.show(block=True)

    return


def main():
    from argparse import ArgumentParser
    import logging
    import sys
    from VHF.log_utils import no_matplot

    argp = ArgumentParser(prog="plot_vhf", description="Plots VHF_v1_parser files.")
    argp.add_argument("-d", "--debug", action="store_true", help="Prints logger to stdout")
    args = argp.parse_args()

    if args.debug:
        logger = logging.getLogger()
        logger.setLevel(logging.DEBUG)
        fmtter = logging.Formatter(
            '[%(asctime)s%(msecs)d] (%(levelname)s) %(name)s:%(funcName)s - \t %(message)s', datefmt='%H:%M:%S:')
        streamhandler = logging.StreamHandler(sys.stdout)
        streamhandler.addFilter(no_matplot)
        streamhandler.setLevel(logging.DEBUG)
        streamhandler.setFormatter(fmtter)
        logger.addHandler(streamhandler)
        logger.debug("run and plot started")
    else:
        logger = logging.getLogger()

    run_and_plot()


if __name__ == "__main__":
    main()
