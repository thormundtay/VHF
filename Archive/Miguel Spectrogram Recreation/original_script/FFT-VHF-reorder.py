# Reordered FFT-VHF.py in accordance with what Miguel has ran to produce
# Figure 37, as detailed in his whatsapp
import os, sys
from pathlib import Path
module_path = str(Path(__file__).parents[3])
if module_path not in sys.path:
    sys.path.append(module_path)
from datetime import timedelta
import numpy as np
import matplotlib.pyplot as plt
from numpy.fft import fft, fftfreq
from obspy import read, UTCDateTime as UTC
from obspy.core.trace import Trace as tr
from obspy import UTCDateTime
import gc
from scipy.fft import fftshift as sss
from obspy.signal.trigger import plot_trigger
from obspy.signal.trigger import classic_sta_lta
from obspy.signal.trigger import z_detect
from obspy.signal.trigger import carl_sta_trig
from obspy.signal.trigger import trigger_onset
from VHF.parse import VHF_v1_parser as VHFparser

def block_avg(my_arr: np.ndarray, N: int):
    """Returns a block average of 1D my_arr in blocks of N."""
    if N == 1:
        return my_arr
    return np.mean(my_arr.reshape(np.shape(my_arr)[0]//N, N), axis=1)

def block_avg_tail(my_arr: np.ndarray, N: int):
    """Block averages the main body, up till last block that might 
    contain less than N items. Assumes 1D."""
    if N == 1:
        return my_arr
    if np.size(my_arr) % N == 0:
        return block_avg(my_arr, N)
    else:
        result = np.zeros(np.size(my_arr)//N + 1)
        result[:-1] = block_avg(my_arr[:np.size(my_arr)//N * N], N)
        result[-1] = np.mean(my_arr[np.size(my_arr)//N * N:])
        return result

def helper(sampless, avg, sample_rate, skip):
    set2 = []
    z = 1 / sample_rate * avg * skip
    for element in sampless:
        set2.append([element[0]*z, element[1] * z])
    return set2

def join(times, joinp):
    tot = 1
    time = times[0][0] #Extracting the info from the data dict.
    times2 = []
    ends = []
    duration = 0
    full = []
    for i in times:
        if i[0] > (time + duration + joinp): #Join events separated by less than 2 sec
            tot += 1
            times2.append(time)
            ends.append(time + duration)
            full.append((time, time + duration))
            time = i[0]
            duration = i[1] - time
        else:
            duration = i[1] - time
    times2.append(time)
    ends.append(time + duration)
    full.append((time, time + duration))
    return tot, times2, ends, full

### Set parameters 0.5771A laser 2  0.5686A laser 4   0.5622  0.5894
low_freq = 1e3
high_freq = 70e3
file_name = "2023-07-06T14:04:22.913767_s499_q268435456_F0_laser2_3674mA_20km"

##### Data extraction
file_rel_path = '../Data/' + file_name + '.bin'
parsed = VHFparser(os.path.join(os.path.dirname(__file__), file_rel_path))
phase = parsed.reduced_phase # Miguel did not realise the /2pi reduction
beggining_time = '2023-07-06T14:04:22.913767'
# phase = phase[10000:]
sample_rate = 2e4 #1/5 of 10Mhz as s4 or less depending on sampling config
num_points = len(phase)
print("I could parse the file!")

#Phase average calulation
phase2 = block_avg_tail(phase, avg:=5)
num_points2 = len(phase2)
print('average done')


#Velocity and phase plots
skip = 1 #in case it is needed to skip every 'skip' samples
wavelength = 1550 *10**(-9) #meters
distance = wavelength * phase2 #meters
#velocity = [0]
timediff = 1 / sample_rate *avg
time = [0]
timer = 0
'''
for i in range(num_points2 - 1):
    try:
        velocity.append((distance[i + 1] - distance[i])/timediff)
        timer += timediff
        time.append(timer)
    except:
        timer += timediff
'''
velocity = (distance[1:] - distance[:-1])/timediff
#To avoid the error on the first 3 values of the acceleration and velocity
for i in range(5):
    velocity[i] = 0
'''
figure1 = plt.figure()
plt.plot(time, velocity)
plt.title('Speed over time')
plt.xlabel(f'Time after {beggining_time} in seconds')
plt.ylabel('Speed (m/s)')
plt.show()
'''

'''
figure3 = plt.figure()
plt.plot(np.arange(num_points) / sample_rate, phase, linewidth = 0.2)
plt.title('Phase over time')
plt.xlabel(f'Time after {beggining_time} in seconds')
plt.ylabel('Phase (1/2pi)')
plt.show()
print('initial plots print')
'''


figure4 = plt.figure()
plt.plot(np.arange(len(phase2)) / sample_rate * avg, phase2, linewidth = 0.2)
plt.title('Phase over time avg')
plt.xlabel(f'Time after {beggining_time} in seconds')
plt.ylabel('Phase (1/2pi)')
plt.show(block=True)
print('initial plots print')


#Velocity saved as a data stream for the obspy
velocity2 = velocity[:: skip]
velocityarray = np.array(velocity2)
num_points3 = len(velocityarray)
startime = UTCDateTime(beggining_time)
endtime = startime + 6000
trr = tr(data = velocityarray)
trr.stats.delta = timediff * skip
trr.stats.starttime = beggining_time
trr.stats.network = 'QITLAB'
trr.stats.station = 'LASER'
trr.stats.location = 'NUS'
trr.stats.channel = 'LowSamp'
#trr._rtrim(trr.stats.starttime + 50)  
trr.plot(handle = True)
#trr.spectrogram(cmap = 'terrain')

figure3 = plt.figure()
plt.plot(np.arange(num_points3)*timediff, velocity2, linewidth = 0.2)
plt.title('Velocity over time')
plt.xlabel(f'Time after {beggining_time} in seconds')
plt.ylabel('Velocity (m/s)')
plt.show(block=True)
print('initial plots print')

'''
trr.filter('bandpass', freqmin=0.5, freqmax=20, zerophase = True)
trr.plot(handle = True)
'''

j = 1
event = (200, 700)
j1 = trr.copy()
begg = startime + event[0] - 15 # This number can change if looking for further/closer earthquakes 
fin = startime + event[1] + 15
j1.trim(begg, fin) #Now we need to cut some of the data as the spectrogram function consumes a lot of memory space
datar = block_avg_tail(j1.data, avg2:=10)
print(f'average done event {j}')
j1 = tr(data = datar)
j1.stats.delta = timediff * skip * avg2
j1.stats.starttime = begg
j1.stats.network = 'QITLAB'
j1.stats.station = 'LASER'
j1.stats.location = 'NUS'
j1.stats.channel = f'Event {j}'
print( f'Event {j} started at {startime + event[0]} approximately and lasted {event[1] - event[0]} seconds.')
j1.spectrogram(cmap = 'terrain', wlen=j1.stats.sampling_rate / 100.0, per_lap=0.90)
# j1.spectrogram(cmap = 'terrain', outfile= 'tmp.png' )
j1.plot(handle = True)
print('\n')
j += 1

my_fig, my_ax = plt.subplots()
j1.spectrogram(axes=my_ax, cmap = 'terrain', wlen=j1.stats.sampling_rate / 100.0, per_lap=0.90)
my_ax.set_ylim(0, 30)
my_fig.savefig('tmp.png')


'''
num_points4 = len(phase2)
startime = UTCDateTime(beggining_time)
endtime = startime + 6000
trr2 = tr(data = phase2)
trr2.stats.delta = timediff * skip
trr2.stats.starttime = beggining_time
trr2.stats.network = 'QITLAB'
trr2.stats.station = 'LASER'
trr2.stats.location = 'NUS'
trr2.stats.channel = 'LowSampPhase'
#trr._rtrim(trr.stats.starttime + 50)  


trr2.plot(handle = True)
trr2.filter('bandpass', freqmin=0.5, freqmax=5000, zerophase = True)#modify this for 2MHz vs 20kHz
''''''
trr2.plot(handle = True)
#Earthquake detection methods
phase3 = trr2.data
'''
'''
cft3 = classic_sta_lta(trr.data, int(5), int(70000)) #Data is extracted as sample number for the triggers  70000 for 2MHz and 
#plot_trigger(trr, cft3, 100, 0.1)
on1 = trigger_onset(cft3, 100, 0.1)
set1time = helper(on1, avg, sample_rate, skip)
set1init = []
joinparameter = 2 #indicates how many seconds can go by between two events that are in reality the same
if len(set1time) != 0:
    total_earthclas, set1init, set1end, notused1 = join(set1time, joinparameter)
    print(f'The classic sta-lta method registers {total_earthclas} possible events')
else:
    print(f'The classic sta-lta method did not register any possible events')
cft4 = z_detect(trr.data, int(70)) #Process repeated for next two methods.
on2 = trigger_onset(cft4, 20, 0.1)
#plot_trigger(trr, cft4, 20, 0.1)
set2time = helper(on2, avg, sample_rate, skip)
set2init = []
if len(set2time) != 0:
    total_earthz, set2init, set2end, notused2 = join(set2time, joinparameter)
    print(f'The z detect method registers {total_earthz} possible events')
else:
    print(f'The z detect method did not register any possible events')



#Combine the measurements, of 2 of the methods
final_events = []
tracker = 0
for i in range(len(set1init)):
    begin = set1init[i]
    end = set1end[i]
    j = tracker
    while j < len(set2init):
        begin2 = set2init[j]
        end2 = set2end[j]
        if (begin - 1) <= begin2 and end2 <= (end + 1):
            final_events.append((begin2, end2))
            tracker = j
        elif (begin - 1) <= begin2 and begin2 <= (end + 1):
            final_events.append((begin2, end))
            tracker = j
        elif end2 <= (end + 1) and (begin - 1) <= end2:
            final_events.append((begin, end2))
            tracker = j
        elif (begin - 1) >= begin2 and end2 >= (end + 1):
            final_events.append((begin, end))
            tracker = j
        elif end <= begin2:
            j = len(set2init)
        j += 1

if len(final_events) == 0:
    print('No agreements between methods')
else:
    #No we join the events inside final_events as the comparisson between sets may have separated them again
    joinparameter2 = 3
    tot, beginfin, endfin, events = join(final_events, joinparameter2)
    print(f'The total {tot} possible events are:')
    j = 1
    for event in final_events: 
        j1 = trr.copy()
        begg = startime + event[0] - 15 # This number can change if looking for further/closer earthquakes 
        fin = startime + event[1] + 15
        j1.trim(begg, fin) #Now we need to cut some of the data as the spectrogram function consumes a lot of memory space
        datas = j1.data
        data = []
        n = 0
        avg2 = 10 # average desired
        while n < len(datas):
            if (n + avg2 - 1) < len(datas):
                tot = 0
                for i in range(avg2):
                    tot += datas[n + i]
                data.append(tot / avg2)
            else:
                tot = 0
                count = 0
                while n < len(datas):
                    count += 1
                    tot += datas[n]
                    n += 1
                if count != 0:
                    data.append(tot / count)
            n += (avg2)
        datar = np.array(data)  
        print(f'average done event {j}')
        j1 = tr(data = datar)
        j1.stats.delta = timediff * skip * avg2
        j1.stats.starttime = begg
        j1.stats.network = 'QITLAB'
        j1.stats.station = 'LASER'
        j1.stats.location = 'NUS'
        j1.stats.channel = f'Event {j}'
        print( f'Event {j} started at {startime + event[0]} approximately and lasted {event[1] - event[0]} seconds.')
        #j1.spectrogram(cmap = 'terrain')
        #j1.plot(handle = True)
        print('\n')
        j += 1
'''
'''
fig5 = plt.figure()
plt.title(f'FFT of {file_rel_path} frequencies speed')
plt.specgram(x = phase, Fs = sample_rate)
plt.show()
'''
'''

#FFT of phase with average
low_freq = 0.3
high_freq = 100000

fft_result2 = fft(phase2)
print("I could Perform FFT!")

# Calculate the frequency values
freqs2 = fftfreq(num_points2, d=1/sample_rate * avg)
print("I could Calculate the frequency values!")

# Apply filter
indices2 = np.where((freqs2 >= low_freq) & (freqs2 <= high_freq))
print("I could Apply filter!")

fft_result2 = fft_result2[indices2]
freqs2 = freqs2[indices2]
print("I could filter fft!")
plt.figure(figsize = (12, 6))
#plt.plot(freqs, np.abs(fft_result))
plt.xlabel('Freq2 (Hz)')
plt.ylabel('FFT Amplitude |X(freq)|')
plt.loglog(freqs2,np.abs(fft_result2))

# q = 82500000
# meassurement_time = 1/(2 * 1e6) * q
plt.title(f'FFT of {file_rel_path} frequencies in time')
plt.grid(True)
plt.show()

'''
