use pariter::IteratorExt;
use std::path::PathBuf;
use vhf::Result;
use vhf::runner::{
    Config, VHF,
    fold::StreamFold,
    writer::{V1Writer, VHFWriter},
};

fn main() -> Result<()> {
    let _ = log4rs::init_file("log4rs.yml", Default::default()).expect("log4rs.yml not found!"); // Logger init
    let conf = Config::new(Some(PathBuf::from("./VHF_board_params.ini")))?;
    conf.inform_params();

    let params = conf.stream_fold_parameters().clone();
    let mut vhf = VHF::new(&conf, &params)?;
    let time_start = vhf.start()?;

    let file_writer = &mut V1Writer::new(&conf, time_start);
    let StreamFold::Map(params) = params else {
        log::error!("Overlapping windows are not StreamFold::Map variant.");
        panic!()
    };

    let vhf_iter = vhf.iter();
    let body = pariter::scope(|scope| {
        vhf_iter
            .step_by(params.step_by)
            .parallel_map_scoped(scope, |x| (*params.func)(x))
            .try_for_each(|write_block| file_writer.write_data(write_block))
            .expect("Failed to write data");
    });

    match body {
        Ok(_) => log::info!("Run completed"),
        Err(e) => log::error!("Main loop occurred with error = {:?}", e),
    };

    // VHF cleanup
    vhf.stop()?;

    file_writer.close()?;

    Ok(())
}
