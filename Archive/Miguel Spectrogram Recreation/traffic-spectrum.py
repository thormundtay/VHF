# With the generation of Figure 37 in Miguel's report much better understood,
# it is now possible to generate a array-cropped output to pass to
# GNUPlot/matplotlib for SVG format.

from datetime import timedelta
from downsamplers import block_avg_tail
from matplotlib import gridspec
from matplotlib import patches
from matplotlib import pyplot as plt
from matplotlib import transforms
import numpy as np
from obspy_spectrogram_modified import spectrogram as spectrogram_data_array
from pathlib import Path
# from pynverse import inversefunc
import sys
from time import perf_counter_ns
module_path = str(Path(__file__).parents[2])
if module_path not in sys.path:
    sys.path.append(module_path)
from VHF.parse import VHF_v1_parser as VHFparser
from VHF.spec.utils import spectrogram_crop, trunc_cmap


def main():
    base_dir = Path(__file__).parent
    data_dir = base_dir.joinpath('Data')

    sparse_num_phase = 5
    sparse_num_velocity = 10

    file1 = data_dir.joinpath('2023-07-06T14:04:22.913767_s499_q268435456_F0_laser2_3674mA_20km.bin')
    parsed1 = VHFparser(
        file1,
        plot_start_time=timedelta(seconds=150),
        plot_duration=timedelta(seconds=400),
    )
    phase1 = parsed1.reduced_phase

    phase1_avg = block_avg_tail(phase1, sparse_num_phase)
    velocity1 = np.diff(phase1_avg) * (parsed1.header["sampling freq"]/sparse_num_phase) # * 1550e-9
    velocity1_avg = block_avg_tail(velocity1, sparse_num_velocity)

    # account for butchering of parsed headers
    parsed1.header["sampling freq"] /= (sparse_num_phase * sparse_num_velocity) # after the dust has settled

    Sxx, f, t, ax_extent = spectrogram_data_array(
        velocity1_avg,
        parsed1.header['sampling freq'],
        wlen=parsed1.header['sampling freq']/100,
        per_lap=0.90  # Following miguel's default
    )

    Sxx, f, t, spec_ax_extent = spectrogram_crop(Sxx, f, t, ax_extent, lambda f: f<=30, lambda t: t<=350)
    # Sxx, f, t, spec_ax_extent = spectrogram_crop(Sxx, f, t, ax_extent, None, None)
    print(f"[Cmap mapping] {np.max(Sxx) = }, {np.min(Sxx) = }")

    matplotlib = True
    if matplotlib:

        # my_norm = colors.FuncNorm((c_forward, c_inv), vmin=180, vmax=440)
        my_norm = None

        # Plot
        # 1. Take a relevant slice of the color bar so that the truncated
        # spectrogram has similar light streaks to what Miguel has done
        # 2. Specify that all values above 440 for vmax are to be mapped to the
        # upper end of cmap.
        # 3. aspect = auto to allow for set_xlim, set_ylim to not squash the ax
        # 4. imshow should always be origin="lower" as set by us!
        fig = plt.figure()
        gs = gridspec.GridSpec(
            nrows=2, ncols=2,
            figure=fig,
            wspace=0.1,
            height_ratios=[2, 3], width_ratios=[1, .03]
        )
        ph_ax = plt.subplot(gs[0, 0])
        spec_ax = plt.subplot(gs[1, 0], sharex=ph_ax)
        colb_ax = plt.subplot(gs[1, 1])

        # plot spectrogram
        cmappable = spec_ax.imshow(
            Sxx, cmap=trunc_cmap('terrain', 0, 0.25),
            norm=my_norm, interpolation="nearest",
            vmax=440,
            extent=spec_ax_extent,
            aspect="auto",
            origin="lower",
        )
        spec_ax.grid(False)

        # add colorbar
        plt.colorbar(cmappable, cax=colb_ax, shrink=1.0, extend='max')

        # plot phase(time)
        ph_ax.plot(np.arange(len(phase1))/2e4, phase1/1e3,
            color='#0000ff', linewidth=0.2)

        # add secondary axis: displacement
        dis_ax = ph_ax.secondary_yaxis('right', functions=(
            lambda x: x * 1.55/1.5,
            lambda x: x / (1.55/1.5)
        ))

        spec_ax.tick_params(axis='both', which='major', labelsize=6)
        colb_ax.tick_params(axis='both', which='major', labelsize=6)
        ph_ax.tick_params(axis='both', which='major', labelsize=6)
        dis_ax.tick_params(axis='both', which='major', labelsize=6)

        spec_ax.set_xlabel('Time (s)', fontsize=6)
        spec_ax.set_ylabel('Frequency (Hz)', fontsize=6)
        colb_ax.set_ylabel('Intensity (a.u.)', fontsize=6)
        ph_ax.set_ylabel(r'$\phi_d$ ($2\pi \cdot 10^3$ rad)',
                         fontsize=6, usetex=True)
        dis_ax.set_ylabel(r'$\Delta L$ (mm)', fontsize=6, usetex=True)
        spec_ax.set_xlim(ax_extent[0], 350.0)
        spec_ax.set_ylim(ax_extent[2], 30)

        fig.set_size_inches(fig_width:=0.495*(8.3-2*0.6), 1.0*fig_width) # A4 paper is 8.3 inches by 11.7 inches
        fig.subplots_adjust(left=0.125, bottom=0.095, right=0.880, top=0.955)

        # add labels
        tr = transforms.ScaledTranslation(-27/72, 2/72, fig.dpi_scale_trans)
        ph_ax.text(0.0, 1.0, '(a)', transform=ph_ax.transAxes + tr,
            fontsize='small', va='bottom', fontfamily='sans')
        spec_ax.text(0.0, 1.0, '(b)', transform=spec_ax.transAxes + tr,
            fontsize='small', va='bottom', fontfamily='sans')

        # add circling
        for cent in [(6.8, 10.75), (72, 11.8), (187.8, 10.3), (223, 10.3), (266, 11.74), (300, 12.26), (323, 12.07)]:
            c = patches.Ellipse(cent, width=(ell_w:=18),
                    height=(np.diff(spec_ax.get_ylim())/np.diff(spec_ax.get_xlim()))*ell_w*1.2,
                    linewidth=1, edgecolor='#ff0000', facecolor='none')
            spec_ax.add_patch(c)

        # plt.show(block=True)
        time_start = perf_counter_ns()
        fig.savefig(base_dir.joinpath('spectrum_miguel.png'), dpi=300, format='png')
        time_end = perf_counter_ns()
        plt.close()
        print(f"Save fig took: {(time_end-time_start)/1e6:.3f}ms.")
    
    gnuplot = False
    if gnuplot:
        Sxx = TODO


if __name__ == '__main__':
    main()
