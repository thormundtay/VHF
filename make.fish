#!/usr/bin/fish

set bold "\e[1m"
set un "\e[4m"
set reset "\e[0m"

function main
  switch $argv
    case "help" "--help" "-h"
      show_help
    case "init"
      init
    case "test"
      test_cargo
    case "test_python" "python_test"
      python_test
    case '*'
      echo "Unrecognised command: $argv"
      printf "Consider running `$bold./make.fish help$reset`\n"
  end
end

function show_help
  printf $bold"Bootstrapping VHF for Fish users$reset\n"
  echo ""
  printf $un"Arguments$reset\n"
  echo "help - Shows this message"
  echo "init - For creating the project"
  echo "test - For testing the project (Rust)"
  echo "python_test - For testing the project (Python)"
  echo ""
  printf $un"Environment Variables$reset\n"
  echo "RUSTFLAGS: $RUSTFLAGS"
end

function init
	g++ VHF/board_init/set_device_mode.cpp -O3 -o VHF/board_init/set_device_mode
	sudo chown root:root VHF/board_init/set_device_mode
	sudo chmod +s VHF/board_init/set_device_mode
	ln -sf /dev/usbhybrid0 vhf_board.softlink
	mkdir Log &
	mkdir Data &
	cargo build --release --bin stream && ln -sf target/release/stream run_vhf
	cargo build --release --bin clear-fifo --features clear-fifo && ln -sf target/release/clear-fifo clear_FIFO
	ln -sf target/release/clear-fifo teststream.exec
end

function test_cargo
	cargo build --bin stream
	cargo build --bin clear-fifo --features clear-fifo
	cargo test -q
end

function python_test
  # There are out of tree tests we are ignoring. Then we ignore long-lived tests.
  pytest --ignore-glob=Archive --ignore=test/test_parseBinaryVHFTrace.py --ignore=test/test_IdentifiedProcess.py --ignore=test/test_VHFPool.py
end

main $argv
