from datetime import datetime, timedelta
from matplotlib import pyplot as plt
from matplotlib.dates import DateFormatter, HourLocator, MinuteLocator
from matplotlib.gridspec import GridSpec
import numpy as np
from pathlib import Path
from scipy.signal import decimate
import sys
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)
from VHF.parse import VHF_v1_parser as VHFparser
from VHF.spec.mlab import detrend_linear, cz_spectrogram_amplitude

BASE_DIR = Path(__file__).parent
NIGHT = Path("/mnt/nas-fibre-sensing/20231115_Cintech_Heterodyne_Phasemeter/2024-01-22T04:43:44.331093_laser_chip_ULN00238_laser_driver_M00435617_laser_curr_392.8mA_port_number_5.bin")
DAY = Path("/mnt/nas-fibre-sensing/20231115_Cintech_Heterodyne_Phasemeter/2024-01-22T08:27:26.850997_laser_chip_ULN00238_laser_driver_M00435617_laser_curr_392.8mA_port_number_5.bin")
NIGHT_START = datetime.fromisoformat(NIGHT_RAW:="20240122T06:00")
# DAY_START = datetime(2024, 01, 22, 06, 00, 00)
DAY_START = datetime.fromisoformat(DAY_RAW:="20240122T09:00")
INTERVAL = timedelta(seconds=15*60)
FIG_WIDTH = 0.5*(8.3-2*0.6-0.1)
NIGHT_PHASE_P = VHFparser(NIGHT, headers_only=True)
NIGHT_PHASE_P.update_plot_timing(lazy=True, start=NIGHT_START, duration=INTERVAL)
NIGHT_PHASE = NIGHT_PHASE_P.reduced_phase
DAY_PHASE_P = VHFparser(DAY, headers_only=True)
DAY_PHASE_P.update_plot_timing(lazy=True, start=DAY_START, duration=INTERVAL)
DAY_PHASE = DAY_PHASE_P.reduced_phase

def phase():
    fig, [night, day] = plt.subplots(figsize=(FIG_WIDTH, 0.6*FIG_WIDTH), nrows=2, dpi=300, sharex=True)
    night.tick_params(labelbottom=False)
    night.tick_params(axis='both', which='major', labelsize=6)
    night.tick_params(axis='both', which='minor', labelsize=6)
    day.tick_params(axis='both', which='major', labelsize=6)
    day.tick_params(axis='both', which='minor', labelsize=6)

    # night_t = np.arange(NIGHT_START.isoformat(),
    #                     (NIGHT_START+INTERVAL).isoformat(),
    #                     np.timedelta64(INTERVAL/len(NIGHT_PHASE)),
    #                     dtype='datetime64[s]')
    # day_t = np.arange(DAY_START.isoformat(),
    #                   (DAY_START+INTERVAL).isoformat(),
    #                   np.timedelta64(INTERVAL*1000/len(DAY_PHASE)),
    #                   dtype='datetime64[s]')
    night_t = np.arange(len(NIGHT_PHASE)) / NIGHT_PHASE_P.header["sampling freq"] /60
    day_t = np.arange(len(DAY_PHASE)) / DAY_PHASE_P.header["sampling freq"] /60

    print(f"{night_t.shape = }, {NIGHT_PHASE.shape = }")
    night.plot(night_t, NIGHT_PHASE, linewidth=0.2)
    day.plot(day_t, DAY_PHASE, linewidth=0.2)
    for ax in [night, day]:
        ax.set_ylabel(r"$\phi_d$/2$\pi$ rad", usetex=True, fontsize=6)
    #     ax.xaxis.set_major_locator(MinuteLocator(interval=5))
    #     ax.xaxis.set_major_formatter(DateFormatter("%H:%M"))
    # fig.autofmt_xdate()
    day.set_xlabel(r"$t$/mins", usetex=True, fontsize=6)
    fig.tight_layout(pad=1.01)
    fig.subplots_adjust(hspace=0.02)
    fig.savefig(BASE_DIR.joinpath("INDIV_PHASE.png"))

def specgram():
    """Side by side spectrograms comparing day and night coloring."""
    sparse_num_phase = 5
    sparse_num_velocity = 10
    sparse_phase_factor = sparse_num_phase
    sparse_velocity_factor = sparse_num_velocity

    day_phase_decimated = decimate(DAY_PHASE, sparse_phase_factor, ftype="fir")
    day_phase_avg_decimated = decimate(day_phase_decimated, sparse_velocity_factor, ftype="fir")
    night_phase_decimated = decimate(NIGHT_PHASE, sparse_phase_factor, ftype="fir")
    night_phase_avg_decimated = decimate(night_phase_decimated, sparse_velocity_factor, ftype="fir")

    daySxx, f, t, day_ax_extent = cz_spectrogram_amplitude(
        signal=day_phase_avg_decimated,
        fn=(2,31),
        samp_rate=DAY_PHASE_P.header['sampling freq']/(sparse_velocity_factor*sparse_phase_factor),
        # wlen=parsed1.header['sampling freq']/100,
        win_s = 6.0,
        p_overlap=0.90,  # Following miguel's default
        detrend_func=detrend_linear,
    )
    day_ax_extent = extent_in_mins(day_ax_extent)
    print(f"{np.max(daySxx) = }")
    nightSxx, f, t, night_ax_extent = cz_spectrogram_amplitude(
        signal=night_phase_avg_decimated,
        fn=(2,31),
        samp_rate=NIGHT_PHASE_P.header['sampling freq']/(sparse_velocity_factor*sparse_phase_factor),
        # wlen=parsed1.header['sampling freq']/100,
        win_s = 6.0,
        p_overlap=0.90,  # Following miguel's default
        detrend_func=detrend_linear,
    )
    night_ax_extent = extent_in_mins(night_ax_extent)
    print(f"{np.max(nightSxx) = }")
    print(f"{night_ax_extent = }")

    fig = plt.figure(figsize=(FIG_WIDTH, 0.6*FIG_WIDTH), dpi=300)
    gs = GridSpec(
        nrows=1, ncols=3,
        figure=fig,
        wspace=0.1,
        # height_ratios=[2, 3],
        width_ratios=[1, 1, .03]
    )
    night = plt.subplot(gs[0])
    day = plt.subplot(gs[1], sharey=night)
    colb = plt.subplot(gs[2])

    night.tick_params(axis='both', which='major', labelsize=6)
    night.tick_params(axis='both', which='minor', labelsize=0)
    day.tick_params(axis='both', which='major', labelsize=6)
    day.tick_params(axis='both', which='minor', labelsize=0)
    day.tick_params(labelleft=False)
    cmap = night.imshow(
        nightSxx,
        vmax=2,
        cmap="gray_r",
        **night_ax_extent
    )
    day.imshow(
        daySxx,
        vmax=2,
        cmap="gray_r",
        **day_ax_extent
    )
    plt.colorbar(cmap, cax=colb, shrink=1.0, extend="max")
    night.set_ylabel(r"$f$/Hz", usetex=True, fontsize=6)
    night.set_xlabel(r"$t$/mins", usetex=True, fontsize=6)
    day.set_xlabel(r"$t$/mins", usetex=True, fontsize=6)
    colb.set_ylabel(r"Amplitude Spectrum Density $\Delta\lambda/\sqrt{\text{Hz}}$", fontsize=6)
    colb.tick_params(axis='both', which='major', labelsize=6)
    # fig.subplots_adjust(wspace=0.04)
    fig.subplots_adjust(left=0.115, bottom=0.165, right=0.850, top=0.945, wspace=0.04)
    fig.savefig(BASE_DIR.joinpath("INDIV_SPEC.png"))

def extent_in_mins(ex: dict):
    extent = ex["extent"]
    extent = (extent[0]/60, extent[1]/60, *extent[2:])
    ex["extent"] = extent
    return ex


def main():
    phase()
    specgram()

    return


if __name__ == "__main__":
    main()
