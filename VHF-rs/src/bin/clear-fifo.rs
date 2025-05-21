use vhf::runner::board::find_device_by_sys;
use vhf::Result;

fn main() -> Result<()> {
    let _ = log4rs::init_file("log4rs.yml", Default::default()).expect("log4rs.yml not found!"); // Logger init

    let boards = find_device_by_sys();

    println!("{:?}", boards);

    Ok(())
}
