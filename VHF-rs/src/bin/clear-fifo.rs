use clap::{ArgAction, Parser};
use log::LevelFilter;
use log4rs::Config;
use log4rs::append::console::ConsoleAppender;
use log4rs::config::{Appender, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use prettytable::format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR;
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

/// Prints out list of devices for selecting to clear.
fn summary_table(boards: &[PathBuf], verbose: bool) -> Result<()> {
    let mut tbl = Table::new();
    tbl.set_format(*FORMAT_NO_BORDER_LINE_SEPARATOR);

    let mut header = vec!["idx", "Serial NO", "In Use", "USB Mode", "Interface"];
    if verbose {
        header.push("Udev address");
    }
    tbl.set_titles(Row::new(
        header.into_iter().map(Cell::new).collect::<Vec<_>>(),
    ));

    boards
        .iter()
        .cloned()
        .map(|p| Board::new(p).unwrap())
        .enumerate()
        .try_for_each(|(i, b)| {
            let mut result = Vec::with_capacity(5 + if verbose { 1 } else { 0 });

            result.push(Cell::new(i.to_string().as_str()));
            result.push(Cell::new(b.board_id.as_str()));
            result.push(Cell::new(if b.in_use()? { "True" } else { "False" }));
            result.push(Cell::new(b.usb_mode()?.to_string().as_str()));
            result.push(Cell::new(
                b.interface_path()?
                    .into_os_string()
                    .into_string()
                    .map_err(|_| Error::ParseUnrecognised("Interface Path".to_string()))?
                    .as_str(),
            ));
            if verbose {
                result.push(Cell::new(
                    b.hotplug_path()?
                        .into_os_string()
                        .into_string()
                        .map_err(|_| Error::ParseUnrecognised("Udev Address".to_string()))?
                        .as_str(),
                ));
            }

            tbl.add_row(Row::new(result));
            Ok::<(), Error>(())
        })?;

    tbl.printstd();
    println!();

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
    tbl.set_format(*FORMAT_NO_BORDER_LINE_SEPARATOR);
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

/// A `Vec<usize>` containing the successfully parsed user input.
fn get_user_selection(upper: usize) -> Result<Vec<usize>> {
    let mut all_parsed_successfully = false;
    let mut numbers = Vec::new();

    while !all_parsed_successfully {
        print!("Please enter boards to clear (space or comma separated): ");

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).map_err(Error::Io)?;
        println!();

        let input = input.trim();
        let parts: Vec<&str> = if input.contains(',') {
            input.split(',').collect()
        } else {
            input.split_whitespace().collect()
        };
        match parts
            .into_iter()
            .map(|p| p.trim())
            .filter(|p| p.is_empty())
            .try_for_each(|tp| tp.parse::<usize>().map(|num| numbers.push(num)))
        {
            Ok(()) => {
                all_parsed_successfully = true;
            }
            Err(_) => {
                numbers = Vec::new();
                all_parsed_successfully = false;
                println!("Unable to parse input. Please try again.")
            }
        }
    }

    Ok(numbers.into_iter().filter(|v| *v < upper).collect())
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

    summary_table(&boards, cli.verbose)?;
    if cli.status {
        show_all_dev_symlinks()?;
        return Ok(());
    }

    let boards: Vec<_> = if boards.len() <= 1 {
        boards
    } else {
        let idx = get_user_selection(boards.len())?;
        let mut select = vec![false; boards.len()];
        idx.into_iter().for_each(|j| select[j] = true);

        boards
            .into_iter()
            .zip(select)
            .filter_map(|(b, s)| if s { Some(b) } else { None })
            .collect()
    };

    for board in boards {
        clear_fifo_per_board(board, true)?;
    }

    show_all_dev_symlinks()?;
    Ok(())
}
