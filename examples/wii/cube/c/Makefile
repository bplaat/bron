NAME := cube
include ../../../../tools/wii-toolchain/wii.mk

target/cube.tpl: src/assets/cube.scf src/assets/crate.jpg | $(TOOLCHAIN_BUILD)/libogc.a
	@mkdir -p target
	cd target && $(abspath $(GXTEXCONV)) -s ../src/assets/cube.scf -o cube.tpl

target/main.o: target/cube.tpl
