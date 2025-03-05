from datetime import datetime, timezone, timedelta
from logging import getLogger
from multiprocessing.pool import Pool
from pathlib import Path
import pytest
import sys
module_path = str(Path(__file__).parents[1])
if module_path not in sys.path:
    sys.path.append(module_path)
from VHF.parse import TraceTimer  # noqa


def log_timer(t: TraceTimer):
    logger = getLogger("test_parse")
    logger.debug("timings.plot_start = %s, timings.plot_end = %s",
                 t.plot_start, t.plot_end)


def test_TraceTimer_regular():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 40000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    expected = timedelta(hours=2)
    assert timings.trace_end - example_start == expected
    assert timings.start_idx == 0
    assert timings.end_idx == 2 * 60 * 60 * 40000
    log_timer(timings)

    rel_start = timedelta(seconds=70)
    change_status = timings.update_plot_timing(start=rel_start)
    expected_duration = timedelta(hours=2) - rel_start
    assert change_status
    assert timings.plot_end - timings.plot_start == expected_duration
    assert timings.plot_start == timings.trace_start + rel_start
    assert timings.plot_end == timings.trace_end
    assert timings.start_idx == 70 * example_freq
    assert timings.end_idx == 2 * 60 * 60 * example_freq
    log_timer(timings)

    new_end_rel_to_trc_start = timedelta(hours=1, seconds=12)
    new_end = example_start + new_end_rel_to_trc_start
    change_status = timings.update_plot_timing(end=new_end)
    expected_duration = new_end_rel_to_trc_start - rel_start
    assert change_status
    assert timings.plot_end - timings.plot_start == expected_duration
    assert timings.plot_start == timings.trace_start + rel_start
    assert timings.plot_end == timings.trace_start + new_end_rel_to_trc_start
    log_timer(timings)

    abs_start = datetime(2024, 1, 19, 1, 34, 36,
                         tzinfo=timezone(timedelta(seconds=28800)))
    dur = timedelta(seconds=5)
    change_status = timings.update_plot_timing(start=abs_start, duration=dur)
    assert change_status
    assert timings.plot_start == abs_start
    assert timings.plot_end - timings.plot_start == dur
    assert timings.duration_idx == int(dur/timedelta(seconds=1/example_freq))
    log_timer(timings)

    change_status = timings.update_plot_timing(start=abs_start, duration=dur)
    assert not change_status


def test_TraceTimer_empty():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 40000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    change_status = timings.update_plot_timing(start=None, end=None)
    assert not change_status


def test_TraceTimer_OOB():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 40000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)

    bad_start = example_start - timedelta(seconds=500)
    change_status = timings.update_plot_timing(start=bad_start)
    assert not change_status
    assert timings.plot_start == timings.trace_start

    bad_end = timings.trace_end + timedelta(seconds=500)
    change_status = timings.update_plot_timing(end=bad_end)
    assert not change_status
    assert timings.plot_end == timings.trace_end
    log_timer(timings)

    example_start = datetime(2024, 1, 1, 9, 15, 00,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 40000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    user_start = example_start - timedelta(hours=1, minutes=15)
    user_dur = timedelta(hours=1)
    change_status = timings.update_plot_timing(
        start=user_start, duration=user_dur)
    assert change_status
    assert timings.end_idx == 0
    log_timer(timings)


def test_TraceTimer_mix_OOB():
    example_start = datetime(2024, 1, 1, 9, 15, 00,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 40000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    user_start = example_start - timedelta(minutes=15)
    user_dur = timedelta(hours=1)
    expected_start = example_start
    expected_end = expected_start + timedelta(minutes=45)

    change_status = timings.update_plot_timing(
        start=user_start, duration=user_dur)
    assert change_status
    assert timings.plot_start == expected_start
    assert timings.plot_end == expected_end
    log_timer(timings)


def test_TraceTimer_high_freq():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 2_000_000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    expected = timedelta(hours=2)
    assert timings.trace_end - example_start == expected
    assert timings.start_idx == 0
    assert timings.end_idx == example_size


def test_TraceTimer_very_high_freq():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 80_000_000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    with pytest.raises(ValueError) as e:
        timings = TraceTimer(example_start, example_freq, example_size)


def test_TraceTimer_very_low_freq():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 0.2  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    expected = timedelta(hours=2)
    assert timings.trace_end - example_start == expected
    assert timings.start_idx == 0
    assert timings.end_idx == example_size


def test_TraceTimer_allowable_freq():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))

    def core_test(skip_param: int):
        example_freq = 10_000_000/(skip_param-1)  # Hz
        example_size = example_freq * 2 * 60 * 60  # 2 hours
        timings = TraceTimer(example_start, example_freq, example_size)
        expected = timedelta(hours=2)
        assert timings.trace_end - example_start == expected
        assert timings.start_idx == 0
        assert timings.end_idx == example_size

    with Pool() as p:
        p.imap_unordered(core_test, range(4, 10_000_000))


def test_TraceTimer_illegal_freq():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 3  # Hz # all sampling_frequencies => 10MHz/n
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    with pytest.raises(ValueError) as e:
        timings = TraceTimer(example_start, example_freq, example_size)


def test_TraceTimer_very_long():
    example_start = datetime(2024, 1, 19, 0, 34, 36,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 2_000_000  # Hz
    example_size = (1 << 33) + (1<<7)
    timings = TraceTimer(example_start, example_freq, example_size)
    # expected = timedelta(hours=2)
    # assert timings.trace_end - example_start == expected
    assert timings.start_idx == 0
    assert timings.end_idx == example_size


def test_TraceTimer_run_and_plot():
    start = datetime.fromisoformat("2024-07-22 12:56:17+08:00")
    freq = 2000000.0
    size = 8413185
    timings = TraceTimer(start, freq, size)
    expected = size * int(1e9/freq)

    assert timings._trace_end_ns - int(start.timestamp()*1e9) == expected
    assert timings.start_idx == 0
    assert timings.end_idx == size


def test_TraceTimer_update_start_only():
    example_start = datetime(2024, 1, 1, 9, 15, 00,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 40000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    user_start = example_start - timedelta(minutes=15)
    user_dur = timedelta(hours=1)
    expected_start = example_start
    expected_end = expected_start + timedelta(minutes=45)

    change_status = timings.update_plot_timing(
        start=user_start, duration=user_dur)
    assert change_status
    assert timings.plot_start == expected_start
    assert timings.plot_end == expected_end

    # Now we update only the start, to be more than the prior set end
    user_start = example_start + timedelta(hours=1, minutes=15)
    expected_start = user_start
    expected_end = user_start

    change_status = timings.update_plot_timing(start=user_start)
    assert change_status
    assert timings.plot_start <= timings.plot_end
    assert timings.plot_start == expected_start
    assert timings.plot_end == expected_end

    # Check that start and end are set to trace_end if user_start > trace_end
    user_start = example_start + timedelta(hours=3, minutes=15)
    expected_start = example_start + timedelta(hours=2)
    expected_end = expected_start
    change_status = timings.update_plot_timing(start=user_start)
    assert change_status
    assert timings.plot_start <= timings.plot_end
    assert timings.plot_start == expected_start
    assert timings.plot_end == expected_end


def test_TraceTimer_update_end_only():
    example_start = datetime(2024, 1, 1, 9, 15, 00,
                             tzinfo=timezone(timedelta(seconds=28800)))
    example_freq = 40000  # Hz
    example_size = example_freq * 2 * 60 * 60  # 2 hours
    timings = TraceTimer(example_start, example_freq, example_size)
    user_start = example_start + timedelta(minutes=15)
    user_dur = timedelta(hours=1)
    expected_start = user_start
    expected_end = expected_start + user_dur

    change_status = timings.update_plot_timing(
        start=user_start, duration=user_dur)
    assert change_status
    assert timings.plot_start == expected_start
    assert timings.plot_end == expected_end

    # Now we update only the end, to be less than the prior set start
    user_end = example_start + timedelta(minutes=10)
    expected_start = user_end
    expected_end = user_end

    change_status = timings.update_plot_timing(end=user_end)
    assert change_status
    assert timings.plot_start <= timings.plot_end
    assert timings.plot_start == expected_start
    assert timings.plot_end == expected_end

    # Check that start and end are set to trace_end if trace_start > user_end
    user_end = example_start - timedelta(minutes=10)
    expected_start = example_start
    expected_end = example_start
    change_status = timings.update_plot_timing(end=user_end)
    assert change_status
    assert timings.plot_start <= timings.plot_end
    assert timings.plot_start == expected_start
    assert timings.plot_end == expected_end
