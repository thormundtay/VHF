use clap::{ArgAction, Parser};
use log::LevelFilter;
use log4rs::Config;
use log4rs::append::console::ConsoleAppender;
use log4rs::config::{Appender, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use prettytable::{Attr, Cell, Row, Table, color, row};
use std::path::PathBuf;
use vhf::runner::board::{Board, find_device_by_sys};
use vhf::{Error, Result};

fn clear_fifo_per_board(board: PathBuf, hybrid_clear: bool) -> Result<()> {
    let board = Board::new(board)?;
    if board.in_use()? {
        log::warn!("Board {} already in use. Not resetting.", &board.board_id);
        return Ok(());
    }
    log::info!("Clearing board {}", &board.board_id);

    board.acm_clear()?;
    board.set_hybrid()?;
    if hybrid_clear {
        board.hybrid_clear()?
    }

    if !board.valid_interface_perms()? {
        log::warn!("VHF drivers weren't installed as user but under sudo. Please reinstall.");
    }

    Ok(())
}

/// Gets all symlinks in the current folder and show if the /dev/usbhybridX the symlink is pointing
/// to is connected.
fn show_all_dev_symlinks() -> Result<()> {
    let curr_dir = std::env::current_dir().map_err(Error::Io)?;

    let points_to_dev = curr_dir
        .read_dir()
        .map_err(Error::Io)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_symlink())
        .filter_map(|sym| {
            let canon = sym.canonicalize();
            if let Ok(true) = canon.map(|e| e.starts_with("/dev")) {
                let p = sym.strip_prefix(curr_dir.clone()).unwrap().to_path_buf();
                let c = unsafe { sym.canonicalize().unwrap_unchecked() };
                let e = c.exists();
                Some((p, c, e))
            } else {
                None
            }
        });

    let mut tbl = Table::new();
    tbl.set_titles(row!["Path", "Device", "Connected"]);
    points_to_dev
        .into_iter()
        .map(|t| {
            let p = Cell::new(t.0.as_os_str().to_str().unwrap());
            Row::new(vec![
                if !t.2 {
                    p.with_style(Attr::BackgroundColor(color::BRIGHT_RED))
                } else {
                    p
                },
                Cell::new(t.1.as_os_str().to_str().unwrap()),
                Cell::new(if t.2 { &"true" } else { &"false" }),
            ])
        })
        .for_each(|r| {
            tbl.add_row(r);
        });

    tbl.printstd();

    Ok(())
}

/// Resets FIFO buffer on VHF board, and summarise state of all boards connected.
#[derive(Parser)]
#[command(about, long_about)]
struct Cli {
    /// Do not clear FIFO, only show state of connected boards
    #[arg(short, long, action = ArgAction::SetTrue)]
    status: bool,
    /// Use Hybrid clear
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    force: bool,
    /// Displays more information
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set log level depending on verbosity
    let _ = log4rs::init_config({
        let stdout = ConsoleAppender::builder()
            .encoder(Box::new(PatternEncoder::new(
                "{d(%Y%m%dT%H:%M:%S)} {h({l:.<5})} [{M}] {m}{n}",
            )))
            .build();
        Config::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .logger(Logger::builder().build("stdout", LevelFilter::Debug))
            .build(Root::builder().appender("stdout").build(if cli.verbose {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            }))
            .unwrap()
    })
    .unwrap();

    let boards = find_device_by_sys()?;

    Ok(())
}
