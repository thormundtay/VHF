.PHONY: init clean all

clean:
# Clean children
	$(MAKE) -C VHF/board_init clean

all: init

# Initialisation for end user
init: vhf_board.softlink teststream.exec clear_FIFO
	$(MAKE) -C VHF/board_init
	mkdir Log &
	mkdir Data &

# Links to the default expected USBHybrid board
vhf_board.softlink:
	ln -sf /dev/usbhybrid0 $@

teststream.exec: VHF-rs/src/bin/stream.rs
# Decouple --release flag from Makefile variable
	cargo build --release --bin stream
	ln -sf target/release/stream $@

clear_FIFO: VHF-rs/src/bin/clear-fifo.rs
# Decouple --release flag from Makefile variable
	cargo build --release --bin clear-fifo --features clear-fifo
	ln -sf target/release/clear-fifo $@

# Builds: Cargo will do incremental compilation; will not let Makefile try incremental
BUILD ?= debug
ifeq ($(BUILD), release)
	BUILD_FLAG = "--release "
else
	BUILD_FLAG =
endif

build: stream clear-fifo

.PHONY: stream
stream: VHF-rs/src/bin/stream.rs
	cargo build $(BUILD_FLAG)--bin stream

.PHONY: clear-fifo
clear-fifo: VHF-rs/src/bin/clear-fifo.rs
	cargo build $(BUILD_FLAG)--bin clear-fifo --features clear-fifo
