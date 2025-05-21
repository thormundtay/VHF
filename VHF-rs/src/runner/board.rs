use crate::{Error, Result};
use std::fs;
use std::path::{PathBuf};

const DEVICE_BASE_PATH: &str = "/sys/bus/usb/devices";

/// List all usb-devices whose product description are VHF.
///
/// bash equivalent: `grep -r VHF /sys/bus/usb/devices/*/product`, yielding
///   `/sys/bus/usb/devices/xxx`
pub fn find_device_by_sys() -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(DEVICE_BASE_PATH)
        .map_err(Error::Io)?
        .filter_map(|read_dir_entry| read_dir_entry.ok())
        .map(|dir_entry| dir_entry.path().as_path().join("product"))
        .filter(|p| p.is_file())
        .filter(|product_file| {
            fs::read_to_string(product_file.clone())
                .unwrap_or_default()
                .contains("VHF")
        })
        .map(|p| p.parent().unwrap().to_path_buf())
        .collect())
}
