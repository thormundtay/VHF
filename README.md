# Python wrapper for VHF (svn) commands

[![DOI](https://zenodo.org/badge/618415525.svg)](https://zenodo.org/doi/10.5281/zenodo.13375529)

*Author: Thormund*  
*Date: 22 March 2023*

This folder is a collection of scripts intended for invoking and parsing the 
output of teststream.c executable, as provided from svn/usbhybrid (r14).

Please be very aware of the use of symlinks in this folder.

## Usage

Some of the following information below is outdated, as it has been made
slightly redundant with the makefile, and that `set_device_mode` has been moved
to `VHF/init_board/`, but the information still remains relevant for triage.

---
### Setup

Run the Makefile by typing
```bash
  make init
```

The board can then save files to one's desired locations as specified by
`VHF_board_params.ini`. Any more granularity in logging information can be done
by changing `log4s.yml`. It is strongly recommended to choose the board via
symblink instead of the configuration file, unless you are certain in what
you're doing.

The board, along with running for however many files as one wishes, is invoked either by
```bash
  ./run_vhf
```
or
```bash
  cargo run --release --bin stream
```

Legacy runners are as follows:  
`set_device_mode.cpp` is compiled by the GCC to become the executable
`set_device_mode`. For it to function properly, it is necessary that this
executable has the necessary permissions. Ideally, `ls -la` would show
```
  -rwsr-xr-x 1 root   users <size> <date> <time> set_device_mode
```

This can be done by `chown root` and `chmod u+x` on the file.

### Symbolic Links

Currently, several files utilise symbolic links to the board and streaming
exec, namely, `/dev/usbhybrid0` and `~/programs/usbhybrid/apps/teststream`.

We note that the relevant locations to these may change in the future, and is
therefore the use of a symbolic link helps keep things self-contained.

The symbolic links currently are
```
  vhf_board.softlink -> /dev/usbhybrid0  
  teststream.exec -> ~/programs/usbhybrid/apps/teststream
```

clear_FIFO.py will prompt as to the symblinks locations.

### Red light is active

The VHF board has a red light used to indicate the FIFO being overflowed.
`clear_FIFO.py` has dependencies `set_device_mode`.

For the most part, this script is run to reset the VHF board into a state ready
for collecting 'stream' data.

`run_vhf` will very likely hit a overflow error if the FIFO was not cleared
prior to invoking `run_vhf`.

### Collecting Data

`run_VHF.py` and `run_and_plot.py` are examples you can refer to. Do note that
they currently pull parameter information from `VHF_board_params.ini` to guide
the filename and locations.

### Plotting Data

Requirements.txt provides additional gui things that assists with plotting
process. However, the installed python modules have some build dependencies.
On OpenSUSE, consider
```
  zypper install cairo-devel gobject-introspection-devel
```
prior to
```
  python -m pip install -r requirements.txt
```

Additional details:  
1. ISO datetime  
In the process of plotting data, git-svn-id: svn://usbhybrid@12 introduced
timing information of the command being recorded straight into the data header.
With string format specifier `%F %T %z`, it is not an ISO-8601 compliant
string, but Python 3.11 and 3.12's datetime library seems to accept it. Until
the svn usbhybrid repository solves this issue, a test as provided in
test/test_datetime.py is provisioned, in case.  
Without VSCode, the tests can be runned by navigating into the `test/` folder,
and running `pytest` in the command line. (This assumes that pip has already
installed pytest.)  
**This should be fixed in USBHybrid repository.**

## Synology NAS for Data collection

Majority of mounting details are documented on Confluence.

To check if it has been mounted, use `mount | grep nfs`. An example `/etc/fstab`:  
```
/etc/fstab
-----
192.168.109.131:/volume1/fibre_sensing	/mnt/nas-fibre-sensing	nfs defaults,_netdev,timeo=900,retrans=5,sec=sys,vers=4,x-systemd.automount,x-systemd.mount-timeout=15,x-systemd.idle-timeout=10min,nofail 0 0
```
