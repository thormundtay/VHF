"""
Provides VHFRunner.

VHFRunner is a config parser that aims to simplify subprocess arguments.
"""
import configparser
import logging
from enum import Enum
from os import PathLike
from os.path import realpath
from pathlib import Path
from shlex import quote
import subprocess
import sys
from typing import Any
from typing import IO
from typing import Iterable
from typing import Mapping
from typing import Optional
from typing import Union
from .process import board_in_use

__all__ = ["VHFRunner"]

# TTY
RED = '\x1B[31m'
REDBOLD = '\x1B[31;1m'
REDINV = '\x1b[41m'
BLUE = '\x1B[34m'
RESET = '\x1B[0m'

# Typing
_PATH = Union[str, PathLike, Path]
_FILE = Union[None, int, IO[Any]]
_TXT = Union[bytes, str]


def flatten(items):
    """Yield items from any nested iterable."""
    for x in items:
        if isinstance(x, Iterable) and not isinstance(x, (str, bytes)):
            for sub_x in flatten(x):
                yield sub_x
        else:
            yield x


class EnumWithAttrs(Enum):
    """Factory class."""

    def __new__(cls, *args, **kwds):
        value = len(cls.__members__) + 1
        obj = object.__new__(cls)
        obj._value_ = value
        return obj


class SamplingSpeed(EnumWithAttrs):
    """Sampling frequency used by VHF board."""

    low = 'l', 10e6
    high = 'h', 20e6

    def __init__(self, flag, _freq):
        self.flag = flag
        self.freq = _freq

    @staticmethod
    def from_str(label: str):
        """Provisions a method to convert a string from config.ini to enum."""
        label = label.strip().lower()
        if label.startswith('l'):
            return SamplingSpeed.low
        elif label.startswith('h'):
            return SamplingSpeed.high
        else:
            raise NotImplementedError(
                "SamplingSpeed.fromstr has received unrecognised input: "
                f"{REDBOLD}{label}{RESET}. Accept only 'low' or 'high'.")


class Encode(EnumWithAttrs):
    """Encoding format piped out by VHF board."""

    BIN = 'b', 'bin'
    HEX = 'x', 'hex'
    ASC = 'A', 'txt'

    def __init__(self, flag, ext):
        self.flag = flag
        self.ext = ext

    @staticmethod
    def from_str(label: str):
        """Provisions a method to convert a string from config.ini to enum."""
        label = label.strip().lower()
        match label[:3].lower():
            case "bin":
                return Encode.BIN
            case "hex":
                return Encode.HEX
            case "asc" | "txt" | "tex":
                return Encode.ASC
            case _:
                raise NotImplementedError(
                    "Encode.fromstr has received unrecognised input: "
                    f"{REDBOLD}{label}{RESET}. Accepts only 'binary', "
                    "'hexadecimal', 'ascii', or 'text'")


def stream_location() -> str:
    """Ensures existence of ./target/release/stream before returning."""
    if not Path("target/release/stream").exists():
        logging.warning("Folder has not been inited? Running stream make now!")
        subprocess.run(["cargo", "build", "--bin", "stream", "--release"])

        if not Path("target/release/stream").exists():
            logging.error("Binary still could not be found! Exiting!")
            sys.exit(1)
    return realpath("target/release/stream")


class VHFRunner():
    """Configparses config file into appropriate strings for subprocess."""

    def __init__(self, conf_file: _PATH, force_to_buffer: bool = False,
                 overwrite_properties: Mapping = dict(),
                 strict_use: bool = True):
        """VHFRunner oversees the necessary conditions to pass into
        subprocess for sampling data with VHF board from config file.

        Inputs
        ------
        conf_file: PathLike
            Specifies location of VHF_board parameters file to read from.
            Specifications of .ini are in accordance with Python's configparser
            library.
        force_to_buffer: bool
            If true, disregards conf_file's save_to_file option.
            This will use a tempdir provided by Python.
        strict_use: bool
            Defaults to True. If true, exits running process, to avoid adding
            more commands to existingly running VHF board.
        """
        self.__createLogger()
        self.strict_mode: bool = strict_use

        # Resolve paths automagically with ExtendedInterpoliation
        conf = configparser.ConfigParser(
            interpolation=configparser.ExtendedInterpolation())
        conf.read(conf_file)

        # Board parameters
        self.num_samples = int(eval(conf.get('Board', 'num_samples')))
        self.skip_num = int(eval(conf.get('Board', 'skip_num')))
        self.speed = SamplingSpeed.from_str(conf.get('Board', 'speed'))
        self.encode = Encode.from_str(conf.get('Board', 'encode'))
        # Map board parameters to corresponding VHF exec flag
        self._board_mappable = {'num_samples': 'q', 'skip_num': 's', }
        # Use enum attrs to get corresponding flag
        self._board_unmappable = ['speed', 'encode',]
        self.board_kwargs = {}
        for k, v in conf['Board'].items():
            if k not in (list(self._board_mappable) + self._board_unmappable):
                if k.endswith('_enable'):  # Do not save _enable keys
                    continue

                # x_enable keys determine if to save x key
                if conf.has_option('Board', k+'_enable'):
                    if conf['Board'][k+'_enable'] == 'False' \
                            or conf.getboolean('Board', k+'_enable') is False:
                        continue

                self.board_kwargs[k] = v

        # Comments for file nomenclature
        self.phasemeter_kwargs = dict(conf['Phasemeter Details'].items())

        # Save locations
        self.path = {
            'base_dir': realpath(conf['Paths']['base_dir']),
            'save_dir': realpath(conf['Paths']['save_dir']),
            'vhf_dev': realpath(conf['Paths']['board']),
            'stream': stream_location(),
        }

        # Currently always into a file until ./stream supports stdout
        self.to_file: bool = True
        self.to_temp_dir: bool = force_to_buffer
        self.temp_dir_loc: PathLike = Path("")
        if force_to_buffer:
            self.path['save_dir'] = ""

        # Overwrite properties read from file via script
        if len(overwrite_properties) > 0:
            self._overwrite_attr(overwrite_properties)

    def __createLogger(self):
        self.logger = logging.getLogger('VHFRunner')

    def _overwrite_attr(self, p: Mapping) -> None:
        """
        Overwrite existing known attrs in config file with those in script.

        Defaults to self.board_kwargs for attrs passed via here that are not
        previously instantised via the config file.
        """
        # Set of known keys
        other_sections = ["board_kwargs", "phasemeter_kwargs", "path"]
        rel_k = [list(self._board_mappable), self._board_unmappable]
        rel_k.extend([list(getattr(self, x)) for x in other_sections])
        rel_section = dict(
            [(t, x) for x in other_sections for t in list(getattr(self, x))]
        )

        for key, v in p.items():
            if key in flatten(rel_k):
                # To find in which kwarg to then overwrite in
                for section in rel_k:
                    if key in section:
                        if section in rel_k[:2]:
                            setattr(self, key, v)
                        else:
                            getattr(self, rel_section[key])[key] = v
                        break

            else:
                self.logger.warning(
                    'Unrecognised key in overwrite_properties: %s', key)
                print(f"'overwrite_properties' contains unknown key {key} "
                      "and will be added into self.board_kwargs.")
                self.board_kwargs[key] = v

    def sample_time(self) -> float:
        """Return the amount of time in seconds to perform sampling."""
        return self.num_samples*(1+self.skip_num)/self.speed.freq

    def inform_params(self) -> None:
        """Print enabled configurations."""
        sf = self.speed.freq/(1+self.skip_num)  # sampling freq
        if sf < 1e3:
            print(f"Sampling at {sf:.4f} Hz.")
        elif sf < 1e6:
            print(f"Sampling at {sf/1e3:.4f} kHz.")
        elif sf < 1e9:
            print(f"Sampling at {sf/1e6:.4f} MHz.")
        else:
            print(f"Sampling at {sf} Hz.")

        if 'vga_num' in self.board_kwargs:
            print("Onboard gain has been set to: "
                  f"{BLUE}{self.board_kwargs['vga_num']}{RESET}.")
        if 'filter_const' in self.board_kwargs:
            print("Filter constant has been set to: "
                  f"{BLUE}{self.board_kwargs['filter_const']}{RESET}.")

        print("Phasemeter details used: "
              f"{BLUE}{self.phasemeter_kwargs}{RESET}.")
        print(f"Directory to be saved to {RED}{self.path['save_dir']}{RESET}")

        if self.to_file:
            print(f"Output will be written to {BLUE}files{RESET}.")
        else:
            print(f"Output will be captured from {BLUE}STDIN{RESET}.")

        st = self.sample_time()
        if st < 60:
            print(f"Sampling is expected to take {REDBOLD}{st:.2f} s{RESET}.")
        elif st < 3600:
            print("Sampling is expected to take "
                  f"{REDBOLD}{st/60:.3f} min{RESET}.")
        elif st < 86400:
            print("Sampling is expected to take "
                  f"{REDBOLD}{st/3600:.3f} hours{RESET}.")
        else:
            print("Sampling is expected to take "
                  f"{REDBOLD}{int(st//86400)} days "
                  f"{int((st % 86400)//3600)} hours "
                  f"{((st % 86400) % 3600)/60:.3f} mins{RESET}.")

    def get_params(self) -> list[str]:
        """Obtain the parameter list invoked for VHF board."""
        def s(x): return str(x)
        def qs(x): return quote(str(x))

        def result_mappable_extend(res: list, param: str):
            res.extend(['-'+self._board_mappable[param],
                        s(getattr(self, param))])

        def result_unmappable_extend(res: list, param: str):
            res.extend(['-'+getattr(self, param).flag])

        result = [qs(self.path['stream'])]  # stream executable
        result.extend(['-U', qs(self.path['vhf_dev'])])  # board location
        for m in self._board_mappable:
            result_mappable_extend(result, m)
        for m in self._board_unmappable:
            result_unmappable_extend(result, m)
        # hardcoded x_enabled flags
        if 'vga_num' in self.board_kwargs:
            result.extend(['-G', self.board_kwargs['vga_num']])
        if 'filter_const' in self.board_kwargs:
            result.extend(['-F', self.board_kwargs['filter_const']])
        # other board params
        for k, v in self.board_kwargs.items():
            if k in list(["vga_num", "filter_const"]):
                continue
            result.extend(['-'+k, s(v)])

        return result

    def subprocess_cmd(self) -> list:
        """First argument of subprocess.run(...)."""
        def qs(x) -> str:
            return quote(str(x))

        result = self.get_params()
        # Currently true until ./stream supports stdout
        if self.to_file:
            if ' ' in self.path["save_dir"]:
                # This is the only part of the file name that is user-defined.
                # VHF board tested to have not been able to receive -o flag
                # with spaces in the name, as of SVN version 22. This can be
                # bypassed by using "-o -" to pipe into STDOUT, which can then
                # be passed as STDIN for writing from.
                raise ValueError(
                    "Save directory has spaces included in it. "
                    "Please use a different directory.")

            if not Path(self.path["save_dir"]).exists():
                # Folder creation is not the responsibility of this class.
                raise ValueError(
                    f"Desired save_dir of `{self.path['save_dir']}` could not"
                    " be found.")

            result.extend(
                ['--save_dir', qs(Path(self.path['save_dir']).resolve())]
            )
            result.append('--phasemeter')
            result.extend(
                ["=".join([k, v]) for k, v in self.phasemeter_kwargs.items()]
            )

        return result

    def subprocess_Popen(self) -> dict:
        """All arguments for subprocess.Popen(...)."""
        if self.board_in_use():
            print(
                f"{REDINV}VHF Board {self.path['vhf_dev']} found to be in "
                f"use!{RESET}"
            )
            self.logger.critical(
                "VHF Runner for VHF board to be currently in use.")
            if self.strict_mode:
                print("Exiting!")
                self.logger.critical("Strict mode enabled! Exiting...")
                sys.exit(1)
        result = {
            'args': self.subprocess_cmd(),
            'cwd': self.path['base_dir'],
        }
        self.logger.debug("subprocess_Popen called. Returning %s", result)
        return result

    def subprocess_run(
        self,
        tmp_dir: Optional[_PATH] = None,
        timeout: Optional[float] = None
    ) -> dict:
        """All arguments for subprocess.run(...).

        If writing to stdout instead, user is to provide their own pipe to pass
        into subprocess.run, through `stdout` arg.
        """
        result = {}
        result["check"] = True
        if timeout is None:
            result["timeout"] = 7 + self.sample_time()
        else:
            result["timeout"] = timeout

        if self.to_file and not self.to_temp_dir:
            result['capture_output'] = True
        elif self.to_temp_dir:
            if tmp_dir is None:
                raise ValueError("tmp_dir expected argument!")
            if self.path['save_dir'] != '':
                raise ValueError("save_dir should have been empty from init")
            self.path['save_dir'] = tmp_dir
        else:
            raise Exception("Inconsistent state achieved")
        result.update(self.subprocess_Popen())
        self.logger.debug("subprocess_run called. Returning %s", result)
        return result

    def board_in_use(self) -> bool:
        """Check if the provided board is being used at the moment."""
        return board_in_use(Path(self.path['vhf_dev']))
