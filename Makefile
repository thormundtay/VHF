.PHONY: init test python_test
init:
	g++ VHF/board_init/set_device_mode.cpp -O3 -o VHF/board_init/set_device_mode
	sudo chown root:root VHF/board_init/set_device_mode
	sudo chmod +s VHF/board_init/set_device_mode
	ln -sf /dev/usbhybrid0 vhf_board.softlink
	mkdir Log &
	mkdir Data &
	cargo build --release --bin stream && ln -sf target/release/stream run_vhf
	cargo build --release --bin clear-fifo --features clear-fifo && ln -sf target/release/clear-fifo clear_FIFO
	ln -sf target/release/clear-fifo teststream.exec

test:
	cargo build --bin stream
	cargo build --bin clear-fifo --features clear-fifo
	cargo test -q

python_test:
	# There are out of tree tests we are ignoring. Then we ignore long-lived tests.
	pytest --ignore-glob=Archive --ignore=test/test_parseBinaryVHFTrace.py --ignore=test/test_IdentifiedProcess.py --ignore=test/test_VHFPool.py
