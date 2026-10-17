NAME := blocks
include ../../../../tools/wii-toolchain/wii.mk

# Convert the block images to a TPL texture file with gxtexconv
target/materials.tpl: src/assets/materials.scf $(wildcard src/assets/*.png) | $(TOOLCHAIN_BUILD)/libogc.a
	@mkdir -p target
	cd target && $(abspath $(GXTEXCONV)) -s ../src/assets/materials.scf -o materials.tpl

target/main.o: target/materials.tpl
