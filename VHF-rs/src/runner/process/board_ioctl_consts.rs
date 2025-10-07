//! All constants related to the starting and stopping of VHF board.
use crate::{Error, Result};
use nix;

const IOCBASE: libc::c_int = 0xaa00;
const IOCBASEWR: libc::c_int = 0xaa00;
const START_USB: libc::c_int = 70 | IOCBASE;
#[allow(dead_code)] // This is not used in tests
const STOP_USB: libc::c_int = 71 | IOCBASE;
const TRANSFERRED_BYTES: libc::c_int = 72 | IOCBASEWR;

nix::ioctl_write_int_bad! {
    /// This is to start the board via IOCTL
    usb_ioctl_start, START_USB
}

#[inline]
pub fn ioctl_start(handle: libc::c_int) -> Result<libc::c_int> {
    unsafe { usb_ioctl_start(handle, 0) }.map_err(Error::Ioctl)
}

nix::ioctl_read_bad! {
    /// This is to read integers from the board.
    usb_ioctl_read, TRANSFERRED_BYTES, libc::c_int
}

#[inline(always)]
pub fn ioctl_read(handle: libc::c_int) -> Result<libc::c_int> {
    // 1 is arbitrary
    unsafe { usb_ioctl_read(handle, std::ptr::null_mut::<i32>()) }.map_err(Error::Ioctl)
}

nix::ioctl_write_int_bad! {
    /// This is to the stop the board via IOCTL
    usb_ioctl_end, STOP_USB
}

#[allow(dead_code)] // This is not used in tests
#[inline]
pub fn ioctl_end(handle: libc::c_int) -> Result<libc::c_int> {
    unsafe { usb_ioctl_end(handle, 0) }.map_err(Error::Ioctl)
}
