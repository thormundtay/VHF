use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use vhf::runner::{Config, VHF};
use vhf::{Error, Result};

fn main() -> Result<()> {
    let _ = log4rs::init_file("log4rs.yml", Default::default()).expect("log4rs.yml not found!"); // Logger init
    let conf = Config::new(Some(PathBuf::from("./VHF_board_params.ini")))?;

    let vhf = VHF::new(&conf)?;
    log::info!("VHF Struct created");

    todo!();
    log::info!("VHF started");

    // Open up file for writing into
    let mut file = {
        use std::fs::OpenOptions;
        use std::io::BufWriter;

        // Delete output if it already exists (truncate is supposed to fix this, but ?)
        if std::fs::exists("/dev/shm/rustout.bin").unwrap() {
            std::fs::remove_file("/dev/shm/rustout.bin").unwrap();
        }

        BufWriter::new(
            OpenOptions::new()
                .create(true)
                .write(true)
                .append(false)
                .truncate(true)
                .open("/dev/shm/rustout.bin")
                .map_err(Error::Io)?,
        )
    };

    // Write some header data for Python parse to work
    {
        use byteorder::{LittleEndian, WriteBytesExt};
        let x: Vec<u32> = vec![
            0xcdef0029, 0x123456ab, 0x6f632023, 0x6e616d6d, 0x696c2064, 0x203a656e, 0x6d6f682f,
            0x69712f65, 0x62616c74, 0x6f72702f, 0x6d617267, 0x73752f73, 0x62796862, 0x2f646972,
            0x73707061, 0x7365742f, 0x72747374, 0x206d6165, 0x2f20552d, 0x2f766564, 0x68627375,
            0x69726279, 0x2d203064, 0x36322071, 0x35333438, 0x20363534, 0x3420732d, 0x206c2d20,
            0x2d20622d, 0x20332076, 0x2f206f2d, 0x2f746e6d, 0x2d73616e, 0x72626966, 0x65732d65,
            0x6e69736e, 0x30322f67, 0x35303432, 0x4d5f3631, 0x435f4644, 0x65746e69, 0x322f6863,
            0x30343230, 0x2f393237, 0x34323032, 0x2d38302d, 0x31543530, 0x33313a36, 0x2e38303a,
            0x35373934, 0x6c5f3634, 0x72657361, 0x6968635f, 0x4c555f70, 0x3230304e, 0x6c5f3833,
            0x6972645f, 0x5f726576, 0x3430304d, 0x31363533, 0x68765f37, 0x515f7066, 0x5f32304f,
            0x75635f6c, 0x305f7272, 0x5f41322e, 0x74726f70, 0x6d756e5f, 0x5f726562, 0x746e6963,
            0x2e686365, 0x206e6962, 0x7220230a, 0x726f6365, 0x676e6964, 0x61747320, 0x203a7472,
            0x34323032, 0x2d38302d, 0x31543530, 0x33313a36, 0x2b38303a, 0x30303830, 0x0000000a,
        ];
        x.into_iter()
            .for_each(|x| file.write_u32::<LittleEndian>(x).unwrap());
    }

    // TODO: Use iterator method on VHF to get stream of data, and transform down before passing to
    // BufWriter.
    todo!();

    log::info!("Run completed");

    // VHF cleanup
    vhf.stop()?;

    {
        // File cleanup
        use std::io::Write;
        file.flush().map_err(Error::Io)?;
    }

    Ok(())
}
