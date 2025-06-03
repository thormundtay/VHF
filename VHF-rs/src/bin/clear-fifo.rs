use std::path::PathBuf;
use vhf::runner::board::{Board, find_device_by_sys};
use vhf::{Error, Result};

fn clear_fifo_per_board(board: PathBuf, hybrid_clear: bool) -> Result<()> {
    let board = Board::new(board)?;
    if board.in_use()? {
        log::warn!("Board {} already in use. Not resetting.", &board.board_id);
        return Ok(());
    }

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

fn main() -> Result<()> {
    let _ = log4rs::init_file("log4rs.yml", Default::default()).expect("log4rs.yml not found!"); // Logger init

    let boards = find_device_by_sys();

    println!("{:?}", boards);

    Ok(())
}
