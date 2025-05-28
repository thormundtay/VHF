use crate::{Error, Result};
use nix::unistd::{Group, User};
use std::fs;
use std::path::{Path, PathBuf};

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

/// Asserts some expecatations with regards to the Cpp file.
fn assert_set_device_perms() -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    const SET_DEVICE_MODE: &str = "VHF/board_init/set_device_mode";
    let set_mode = Path::new(SET_DEVICE_MODE);

    let mut has_error: bool = false;

    if !set_mode.exists() {
        log::error!(
            "Compiled C file for setting VHF board could not be found! Run `make init` at root of git repository.",
        );
        return Err(Error::User);
    }

    let metadata = fs::symlink_metadata(set_mode).map_err(Error::Io)?;

    // Check 2: File owner
    if User::from_uid(metadata.uid().into())
        .map_err(Error::CIo)?
        .map(|u| u.name.to_ascii_uppercase())
        != Some(String::from("ROOT"))
    {
        log::error!(
            "Compiled C file for setting VHF board has wrong owner! Run `make init` at root of git repository."
        );
        has_error = true;
    }

    // Check 3: File group
    if Group::from_gid(metadata.gid().into())
        .map_err(Error::CIo)?
        .map(|u| u.name.to_ascii_uppercase())
        != Some(String::from("ROOT"))
    {
        log::error!(
            "Compiled C file for setting VHF board has wrong group! Run `make init` at root of git repository."
        );
        has_error = true;
    };

    const S_ENFMT: u32 = 0o2000; // From Python
    if (metadata.mode() & S_ENFMT) == 0 {
        log::error!(
            "Compiled C file for setting VHF board has wrong permissions bits! Run `make init` at root of git repository."
        );
        has_error = true;
    }

    if has_error {
        return Err(Error::User);
    }

    Ok(())
}
