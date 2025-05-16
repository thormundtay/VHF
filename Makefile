.PHONY: init
init:
	g++ VHF/board_init/set_device_mode.cpp -O3 -o VHF/board_init/set_device_mode
	sudo chown root:root VHF/board_init/set_device_mode
	sudo chmod +s VHF/board_init/set_device_mode
	ln -sf /dev/usbhybrid0 vhf_board.softlink
	ln -sf /home/qitlab/programs/usbhybrid/apps/teststream teststream.exec
	mkdir Log
	mkdir Data
	cargo build --release --bin stream
	ln -sf target/release/stream run_vhf
