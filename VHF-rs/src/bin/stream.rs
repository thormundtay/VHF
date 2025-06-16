use pariter::IteratorExt;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use vhf::runner::{
    Config, VHF,
    fold::StreamFold,
    writer::{V1Writer, VHFWriter, WriteBlock},
};
use vhf::{Error, Result};

/// Start logging and get [vhf::runner::Config] from file and command line.
fn initialisation() -> Result<Config> {
    if env::args_os().any(|s| s == *"-?" || s == *"--help") {
        Config::default().with_cli(env::args_os())?; // Do not parse VHF_board_params if --help
    }

    log4rs::init_file("log4rs.yml", Default::default()).expect("log4rs.yml not found!"); // Logger init
    let conf = {
        let mut result = Config::new(Some(PathBuf::from("./VHF_board_params.ini")))?;
        if env::args_os().len() > 1 {
            result.with_cli(env::args_os())?; // Invoke argument parsing only when arguments are present
        }
        result.is_valid()?;
        result.inform_params();
        result
    };
    Ok(conf)
}

/// Separate file writer into its own child thread.
fn writer_thread(consumer: Receiver<WriteBlock>, mut file_writer: V1Writer) {
    // try_recv will sleep when empty
    while let Ok(words) = consumer.recv() {
        file_writer.write_data(words).expect("failed to write");
    }
    file_writer.close().expect("Failed to close file_writer");
}

fn main() -> Result<()> {
    let conf = initialisation()?;
    let params = conf.stream_fold_parameters().clone();
    let mut vhf = VHF::new(&conf, &params)?;
    let time_start = vhf.start()?;

    let file_writer = V1Writer::new(&conf, time_start);
    let StreamFold::Map(params) = params else {
        log::error!("Overlapping windows are not StreamFold::Map variant.");
        panic!()
    };

    let (writer_send, writer_receive) = channel();
    let writer_thread = thread::Builder::new()
        .name("File Writer".to_string())
        .spawn(move || writer_thread(writer_receive, file_writer))
        .map_err(Error::Io)?;

    let vhf_iter = vhf.iter();
    let body = pariter::scope(|scope| {
        vhf_iter
            .step_by(params.step_by)
            .parallel_map_scoped(scope, |x| (*params.func)(x))
            .try_for_each(|write_block| writer_send.send(write_block))
            .expect("Failed to write data");
    });

    match body {
        Ok(_) => log::info!("Run completed"),
        Err(e) => log::error!("Main loop occurred with error = {:?}", e),
    };

    // VHF cleanup
    vhf.stop()?;

    // File writer clean up
    drop(writer_send);
    writer_thread.join().expect("Could not close writer thread");

    Ok(())
}
