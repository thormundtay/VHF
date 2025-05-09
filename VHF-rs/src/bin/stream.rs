use pariter::IteratorExt;
use std::path::PathBuf;
use vhf::runner::{
    fold::StreamFold,
    writer::{V1Writer, VHFWriter},
    Config, VHF,
};
use vhf::Result;

fn main() -> Result<()> {
    let _ = log4rs::init_file("log4rs.yml", Default::default()).expect("log4rs.yml not found!"); // Logger init
    let conf = Config::new(Some(PathBuf::from("./VHF_board_params.ini")))?;

    let vhf = VHF::new(&conf)?;
    let time_start = vhf.start()?;

    let file_writer = &mut V1Writer::new(&conf, time_start);
    let StreamFold::Map(params) = conf.stream_fold_parameters().clone() else {
        log::error!("Overlapping windows are not StreamFold::Map variant.");
        panic!()
    };

    // TODO: Use iterator method on VHF to get stream of data, and transform down before passing to
    // BufWriter.
    vhf.into_iter()
        .step_by(params.step_by)
        .parallel_map(move |x| (*params.func)(x))
        .try_for_each(|write_block| file_writer.write_data(write_block))?;

    log::info!("Run completed");

    // VHF cleanup
    // Currently invoked from next()
    // vhf.stop()?;

    file_writer.close()?;

    Ok(())
}
