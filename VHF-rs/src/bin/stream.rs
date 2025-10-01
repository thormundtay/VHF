use jiff::Zoned;
use pariter::IteratorExt;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use vhf::runner::writer::WriterBuilder;
use vhf::runner::{Config, VHF, fold::StreamFoldOp, writer::WriteBlock};
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
fn writer_thread(consumer: Receiver<WriteBlock>, config: Config, time_start: Zoned) {
    let builder: WriterBuilder<'_> = config.file_writer().unwrap();
    let builder = builder.with_start_time(time_start);
    let mut file_writer = builder.build();
    // try_recv will sleep when empty
    while let Ok(words) = consumer.recv() {
        file_writer.write_data(words).expect("failed to write");
    }
    file_writer.close().expect("Failed to close file_writer");
}

fn main() -> Result<()> {
    let conf = initialisation()?;
    let board_conf = conf.build_board_config()?;
    let params = board_conf.stream_fold_parameters().clone();
    matches!(params.op, StreamFoldOp::Map(None));
    assert!(conf.file_writer().is_ok());

    let conf_bind = conf.clone();
    let mut vhf = VHF::new(&conf_bind, &params)?;
    let time_start = vhf.start()?;

    let (writer_send, writer_receive) = channel();
    let writer_thread = thread::Builder::new()
        .name("File Writer".to_string())
        .spawn(move || writer_thread(writer_receive, conf, time_start))
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
        Err(e) => log::error!("Main loop occurred with error = {e:?}"),
    };

    // VHF cleanup
    vhf.stop()?;

    // File writer clean up
    drop(writer_send);
    writer_thread.join().expect("Could not close writer thread");

    Ok(())
}
