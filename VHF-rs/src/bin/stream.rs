use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use vhf::runner::{Config, VHF};
use vhf::{Error, Result};

const IOCBASEWR: libc::c_int = 0xaa00;
const TRANSFERRED_BYTES: libc::c_int = 72 | IOCBASEWR;

fn main() -> Result<()> {
    let _ = log4rs::init_file("log4rs.yml", Default::default()).unwrap(); // Logger init
    let conf = Config::new(Some(PathBuf::from("./VHF_board_params.ini")))?;

    let vhf = VHF::new(conf)?;
    log::info!("VHF Struct created");
    log::info!("VHF.handle = {}", vhf.handle);

    vhf.start()?;
    log::info!("VHF started");

    nix::ioctl_read_bad! {
        /// This is to read integers from the board.
        usb_ioctl_read, TRANSFERRED_BYTES, libc::c_int
    }

    // try to get some data
    let mut i = 0;
    let mut old_tbf32 = 0;
    while i < 5 {
        // the actual value also, does not matter, it seems
        let tfb32 = unsafe { usb_ioctl_read(vhf.handle, 1 as *mut i32) }.unwrap();
        let tfb32: usize = tfb32 as usize;
        if tfb32 > old_tbf32 {
            println!("tfb32 = {}", tfb32);
            println!("test_ = {}", vhf.readback[tfb32]);
            sleep(Duration::from_nanos(500));
            old_tbf32 = tfb32;
            i += 1;
        }
    }
    log::info!("Run completed");

    vhf.stop()?;
    log::info!("Stopped");
    Ok(())
}
