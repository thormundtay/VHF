#!/usr/bin/fish

set bold "\e[1m"
set un "\e[4m"
set reset "\e[0m"

set pyenv_venv_ver "3.12.1"
set pyenv_venv_name "o3"
if test -w "/var/compressed/"
  set -x TMPDIR /var/compressed
end

function main
  set cmd $argv[1]

  switch "$cmd"
    case "help" "--help" "-h"
      show_help
    case "init"
      init
    case "test"
      test_cargo $argv[2..]
    case "test_python" "python_test"
      python_test
    case "delete_venv"
      delete_virtualenv
    case "create_venv"
      create_pyenvvirtualenv
    case "run_bind"
      run_bind $argv[2..]
    case "bacon"
      activate_venv
      bacon --watch vhf_parse $argv[2..]
    case "doc" "docs"
      make docs PRIV=y
    case "test_full"
      test_cargo $argv[2..]
      activate_venv
      test_cargo --features=o3 $argv[2..]
      python_test
    case "watch_image"
      function compile_and_display
        set -l f $argv[1]
        make -s docs_images 2> /dev/null 1> /dev/null
        and begin
          if command -sq kitten
            kitten icat $(echo $f | sed "s/.tex\$/.png/") &
          end
        end
        and make -s docs PRIV=y
      end

      inotifywait -P -r --event close_write --event modify --format '%w%f' --monitor VHF-rs/**/images \
      | stdbuf -oL grep -E "/[^.][^/]*\.tex\$" \
      | while read -L -l file
        printf "Change in file: $file\n"
        compile_and_display $file
      end

    case '*'
      echo "Unrecognised command: $argv"
      printf "Consider running `$bold./make.fish help$reset`\n"
      return 1
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
  echo "TMPDIR: $TMPDIR"
  echo "RUSTFLAGS: $RUSTFLAGS"
  echo "PYENV_VIRTUAL_ENV: $PYENV_VIRTUAL_ENV"
  echo "LD_LIBRARY_PATH: $LD_LIBRARY_PATH"
  echo "PYTHONPATH: $PYTHONPATH"
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
	ln -sf target/release/stream teststream.exec
end

function test_cargo
	cargo build --bin stream
	cargo build --bin clear-fifo --features clear-fifo
	if test -z "$argv"
    cargo test -q
	else if test "$argv" = "--features=o3"
    cargo test -q --features=o3
  else
	  cargo test --color=always $argv
  end
  if test $status -ne 0
    echo "Cargo test errored"
    exit $status
  end
end

function python_test
  # There are out of tree tests we are ignoring. Then we ignore long-lived tests.
  pytest --ignore-glob=Archive --ignore=test/test_parseBinaryVHFTrace.py --ignore=test/test_IdentifiedProcess.py --ignore=test/test_VHFPool.py
end

function delete_virtualenv
  pyenv shell $pyenv_venv_ver
  sleep 0.1
  yes | pyenv virtualenv-delete $pyenv_venv_name
end

function create_pyenvvirtualenv
  pyenv shell $pyenv_venv_ver
  if test $status -ne 0
    echo "Failed to activate pyenv shell"
    exit $status
  end
  sleep 0.2
  pyenv virtualenv $pyenv_venv_name
  if test $status -ne 0
    echo "Failed to created pyenv"
    exit $status
  end
  sleep 0.2
  pyenv activate o3
  if test $status -ne 0
    echo "Failed to activate pyenv"
    exit $status
  end
  sleep 0.2
  python -m pip install -r requirements.txt
  if test $status -ne 0
    echo "Warning: Installation of requirements failed"
  end
end

function activate_venv
  pyenv activate $pyenv_venv_name
  if test $status -ne 0
    echo "Could not find pyenv. Trying to build..."
    create_pyenvvirtualenv
  end
  sleep 0.2
  set -l pyo3ld (dirname (dirname $PYENV_VIRTUAL_ENV))
  if not test -d $pyo3ld
    echo "Could not find lib as parent of pvenv"
    exit 1
  end
  set -gx LD_LIBRARY_PATH $pyo3ld/lib
  set -gx PYO3_PYTHON $PYENV_ROOT/shims/python
  set -gx PYTHONPATH $PWD
  switch $argv[1]
    case "help" "--help" "-h"
      echo "Help invoked in activate_venv"
      show_help
      return 0
  end
end

function run_bind
  activate_venv $argv
  cargo run --bin bind-py
end

main $argv
