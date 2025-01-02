use nix::fcntl;
use std::fs;
use std::io::Write; // Write! macro
use std::os::unix::io::FromRawFd; // for raw_fd trait
use std::thread::sleep;
use std::time::Duration;

const IOCBASE: libc::c_int = 0xaa00;
const IOCBASEWR: libc::c_int = 0xaa00;
const START_USB: libc::c_int = 70 | IOCBASE;
const STOP_USB: libc::c_int = 71 | IOCBASE;
const TRANSFERRED_BYTES: libc::c_int = 72 | IOCBASEWR;
// const DMA_BUF_MAGIC: u8 = b'u';

/// Gets handle for USB device
fn open_device() -> Result<std::os::fd::RawFd, nix::errno::Errno> {
    const DEVICE_FILENAME: &str = "/dev/usbhybrid0";

    fcntl::open(
        DEVICE_FILENAME,
        fcntl::OFlag::O_RDWR,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
}

nix::ioctl_write_int_bad! {
    /// This is to start the board via IOCTL
    usb_ioctl_start, START_USB
}

nix::ioctl_read_bad! {
    /// This is to read integers from the board.
    usb_ioctl_read, TRANSFERRED_BYTES, libc::c_int
}

nix::ioctl_write_int_bad! {
    /// This is to the stop the board via IOCTL
    usb_ioctl_end, STOP_USB
}

fn main() {
    let handle: libc::c_int = open_device().expect("VHF board not found");
    println!("handle = {}", handle);

    let raw_handle = &mut unsafe { fs::File::from_raw_fd(handle) };

    // prepare readback buffer
    let mmap_options = unsafe {
        mmap_rs::MmapOptions::new(1 << 22)
            .expect("Mmap new")
            .with_file(raw_handle, 0)
            .with_flags(mmap_rs::MmapFlags::SHARED)
    };
    let rbbuf = mmap_options.map_mut().expect("Map mut");
    let _tmp: u8 = (0usize..(1 << (22 / 4)))
        .step_by(1024)
        .map(|i| rbbuf[i])
        .sum(); // Is this needed?

    println!("start address = {:#x}", rbbuf.start());
    println!("buf size = {:?}", rbbuf.size());
    println!(
        "debug: {:?}",
        mmap_rs::MemoryAreas::open(None)
            .expect("obtain memory area of current proc")
            .next()
            .unwrap()
            .expect("Get the first Memory area")
    );
    println!(
        "debug: protection = {:?}",
        mmap_rs::MemoryAreas::query(rbbuf.start())
            .expect("obtain memory area of requested address")
            .expect("memory area should have been mapped")
    );
    //
    // start USB machine
    // the 0 is basically because a value has to be
    // passed, the actual value is given in ioctl start
    let x = unsafe { usb_ioctl_start(handle, 0) };
    println!("x = {:?}", x);
    if x.is_err() {
        return;
    }

    // stream some startup into device
    write!(raw_handle, "clockinit; adcinit;").expect("start clock");
    write!(raw_handle, "config 16; param 0;").expect("filter config"); // no filter
    write!(raw_handle, "config 1; param 4;").expect("skip constant"); // skips 4 samples
    write!(raw_handle, "config 2; param 0;").expect("gain param"); // Gain parameter = 0
    write!(raw_handle, "config 3; param 0;").expect("debug param"); // debug param = 0
    write!(raw_handle, "skip; skip;").unwrap();
    write!(raw_handle, "cstream {};", 0x120).expect("start stream");

    // try to get some data
    let mut i = 0;
    let mut old_tbf32 = 0;
    while i < 5 {
        // the actual value also, does not matter, it seems
        let tfb32 = unsafe { usb_ioctl_read(handle, 1 as *mut i32) }.unwrap() as usize;
        if tfb32 > old_tbf32 {
            println!("tfb32 = {}", tfb32);
            println!("test = {}", rbbuf[tfb32]);
            sleep(Duration::from_nanos(500));
            old_tbf32 = tfb32;
            i += 1;
        }
    }

    //stop USB machine
    let _ = unsafe { usb_ioctl_end(handle, 0) }.expect("stop usb");
}
