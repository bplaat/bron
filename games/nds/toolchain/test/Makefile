NAME := target/test
BUILD := target
GAME_TITLE := Toolchain test
HEADER_TITLE := TEST
GAME_SUBTITLE1 := nds-ports
GAME_SUBTITLE2 := ./build.sh test
SOURCES := src/main
ITCM_SOURCES := src/itcm
NITRO_DIR := src/nitro
TOOLCHAIN := ..
include ../nds.mk

# Without builtins, so the C library is tested instead of what the compiler works out itself
CFLAGS += -std=gnu23 -Wextra -fno-builtin
