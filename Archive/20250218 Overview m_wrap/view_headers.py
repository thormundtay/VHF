import csv
from matplotlib import pyplot as plt
import numpy as np
from numpy.typing import NDArray
from pathlib import Path
from summarise_all_headers import CSV_FIELDS

BASE_PATH = Path(__file__).parent
SUMMARY_FILE_OLD = BASE_PATH.joinpath("summarized_headers.csv.bak")
SUMMARY_FILE = BASE_PATH.joinpath("summarized_headers.csv")
FIELDS_WITH_TYPE = list(zip(CSV_FIELDS, ["U200", "U10", "<i8", "<i8", "U", "U"]))
# FIELDS_WITH_TYPE = "U, U, <i8, <i8, U, U"


def data_from_backup():
    with open(SUMMARY_FILE_OLD, 'r') as file:
        d_reader = csv.DictReader(file, fieldnames=CSV_FIELDS)
        result = [row for row in d_reader]
        result.pop(0)  # Remove header
        return result


def data_from_backup_as_array() -> NDArray:
    """Read into a numpy array."""
    with open(SUMMARY_FILE, 'r') as file:
        len_d_reader = len(file.readlines()) - 1

    with open(SUMMARY_FILE, 'r') as file:
        d_reader = csv.DictReader(file, fieldnames=CSV_FIELDS)
        result = np.zeros(
            (len_d_reader,),
            dtype=FIELDS_WITH_TYPE
        )
        for i, row in enumerate(d_reader):
            if i == 0:  # Remove header
                continue
            result[i-1] = np.array(
                tuple(row.values()),
                dtype=FIELDS_WITH_TYPE
            )

        return result


def plot_data(data: NDArray):
    """With dict_reader data, we now see the number of data points by ratio."""
    # Raw
    x_raw = np.fromiter(map(lambda x: x[1], data), dtype=FIELDS_WITH_TYPE[1][1])
    ratio = np.fromiter(map(lambda x: x[3]/x[2], data), dtype=np.float64)

    x = {"None", "0km", "500m", "1km", "20km"}
    x_raw_set = set(x_raw)
    if x_raw_set - x:
        print("Error! Data in csv contains fibre lengths not expected.")
        return

    heights = {}
    for key in x:
        heights[key] = (
            ratio[np.where(x_raw == key)].mean(),
            ratio[np.where(x_raw == key)].std(),
            ratio[np.where(x_raw == key)].max(),
        )
    x = tuple([k for k in x if k != "None"])
    print(f"{x = }")
    height = tuple(heights[k][0] for k in x if k != "None")
    error = tuple(heights[k][1] for k in x if k != "None")
    max = tuple(heights[k][2] for k in x if k != "None")

    for k, m, r, e in zip(x, max, height, error):
        print(f"l = {k:>7s}, max = {m:>5.12f}, r = {r:>5.12f}, {e = }")

    # Plot
    fig, ax = plt.subplots(nrows=1, ncols=1)

    ax.bar(x, height)
    ax.errorbar(x, height, error)

    plt.show(block=True)
    return


def clean_and_sort_csv():
    data = data_from_backup()
    data.sort(key=lambda x: x[CSV_FIELDS[0]])

    with open(SUMMARY_FILE, 'w') as csv_file:
        d_writer = csv.DictWriter(csv_file, fieldnames=CSV_FIELDS)
        d_writer.writeheader()
        d_writer.writerows(data)

    return


def plot_csv():
    data = data_from_backup_as_array()
    plot_data(data)


def main():
    clean_and_sort_csv()
    plot_csv()


if __name__ == "__main__":
    main()
