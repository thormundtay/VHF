import datetime
import logging
import subprocess
from matplotlib import pyplot as plt
from pathlib import Path
from plot_VHF_output import plot_rad_spec
from tempfile import TemporaryFile
from VHF.parse import VHFparser
from VHF.runner import VHFRunner


def main():
    logging.basicConfig(
        filename=datetime.datetime.now().strftime(
            "Log/showRstdDEV_%Y%m%d_%H%M%s.log"
        ),
        filemode="w",
        format="[%(asctime)s] %(name)s -\t%(levelname)s -\t%(message)s",
        level=logging.DEBUG,
    )
    
    vhf_config_path = Path(__file__).parent.joinpath('VHF_board_params.ini')
    vhf_runner = VHFRunner(vhf_config_path, force_to_buffer=True,
        overwrite_properties={'num_samples': int(2**18), 'skip_num': 5 - 1})
    vhf_runner.inform_params()

    start_time = datetime.datetime.now()
    try:
        with TemporaryFile() as f:
            retcode = subprocess.run(
                **(sb_run:=vhf_runner.subprocess_run(stdout=f))
            )
            logging.info("Subprocess ran with %s", str(sb_run))
            logging.info("Retcode %s", retcode)
            parsed = VHFparser(f)
            phase = parsed.reduced_phase
            fig = plot_rad_spec(True, False, True)(parsed, phase)

    except KeyboardInterrupt:
        logging.info("Subprocess ran with %s", str(sb_run))
        logging.info("Keyboard Interrupt")
        print("Keyboard Interrupt recieved!")
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        logging.info("Subprocess ran with %s", str(sb_run))
        logging.critical("exc: %s", exc)
        logging.critical("exc.stderr = ", exc.stderr)
        logging.critical("", exc_info=True)
        if isinstance(exc, subprocess.CalledProcessError):
            print(f"Process returned with error code {exc.returncode}")
        print(f"{exc.stderr = }")


if __name__ == "__main__":
    main()
