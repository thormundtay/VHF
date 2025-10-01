#!/usr/bin/bash

shopt -s nocasematch

function main() {
  set_env_vars

  case $1 in
    "help" | "--help" | "-h")
      show_help
      ;;
    "init")
      init
      ;;
    "test")
      test_cargo
      ;;
    "python_test")
      test_python
      ;;
    *)
      echo "Unrecognised argument: $1"
  esac
}

function set_env_vars() {
  if [ -z $LD_LIBRARY_PATH ]; then
    if [ -n "$PYENV_VIRTUAL_ENV" ]; then
      venv_parent="$(dirname $PYENV_VIRTUAL_ENV)"
      export LD_LIBRARY_PATH="$(dirname $venv_parent)/lib"
    fi
  fi

  if [ -z $PYTHONPATH ]; then
    export PYTHONPATH=$PWD
  fi
}

function show_help() {
  bold=$(tput bold)
  un=$(tput smul)
  reset=$(tput sgr0)
  echo "${bold}Bootstrapping VHF${reset}, for Bash users"
  echo "Management of Pyenv Virtual Env activation is not provided by this Bash script."
  echo ""
  echo "${un}Arguments${reset}"
  echo "help - Shows this message"
  echo "init - For creating the project"
  echo "test - For testing the project (Rust)"
  echo "python_test - For testing the project (Python)"
  echo ""
  echo "${un}Environment Variables${reset}"
  echo "RUSTFLAGS: $RUSTFLAGS"
  echo "PYENV_VIRTUAL_ENV: $PYENV_VIRTUAL_ENV"
  echo "LD_LIBRARY_PATH: $LD_LIBRARY_PATH"
  echo "PYTHONPATH: $PYTHONPATH"
}

function init() {
	g++ VHF/board_init/set_device_mode.cpp -O3 -o VHF/board_init/set_device_mode
	sudo chown root:root VHF/board_init/set_device_mode
	sudo chmod +s VHF/board_init/set_device_mode
	ln -sf /dev/usbhybrid0 vhf_board.softlink
	mkdir Log &
	mkdir Data &
	cargo build --release --bin stream && ln -sf target/release/stream run_vhf
	cargo build --release --bin clear-fifo --features clear-fifo && ln -sf target/release/clear-fifo clear_FIFO
	ln -sf target/release/stream teststream.exec
}

function test_cargo() {
	cargo build --bin stream
	cargo build --bin clear-fifo --features clear-fifo
	cargo test -q
}

function python_test() {
  # There are out of tree tests we are ignoring. Then we ignore long-lived tests.
  pytest --ignore-glob=Archive --ignore=test/test_parseBinaryVHFTrace.py --ignore=test/test_IdentifiedProcess.py --ignore=test/test_VHFPool.py
}

main $1
