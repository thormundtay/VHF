CC = gcc
CFLAGS = -O3 -Wall
CXX = g++
CXXFLAGS = -O3 -Wall

SRCFILES = $(wildcard *.c) $(wildcard *.cpp)
TARGETS = $(basename $(SRCFILES))

.PHONY: clean all

all: $(TARGETS)

clean:
	$(RM) $(TARGETS)

%: %.c
	$(CC) $(CFLAGS) $< -o $@

%: %.cpp
	$(CXX) $(CXXFLAGS) $< -o $@

# set_device_mode requires different/correct perms
set_device_mode: set_device_mode.cpp
	$(CXX) $(CXXFLAGS) $< -o $@
	sudo chown root:root set_device_mode
	sudo chmod +s set_device_mode
