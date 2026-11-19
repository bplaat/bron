# Make rules to build a Wii U program with clang and lld, include this from an example Makefile:
#
#   NAME := triangle
#   include ../../../../tools/wiiu-toolchain/wiiu.mk
#
# Then `make` builds $(BUILD)/$(NAME).rpx from src/*.c, `make run` starts it in Cemu and
# `make bundle` packs it with the names and images of meta/ into $(BUILD)/$(NAME).wuhb for the
# Wii U Menu. The shaders in src/shader.vsh and src/shader.psh are assembled to
# $(BUILD)/shader.gsh, a src/assets/textures.toml config is converted to $(BUILD)/textures.gtx with
# gtxconv.

TOOLCHAIN := $(patsubst %/,%,$(dir $(lastword $(MAKEFILE_LIST))))
TOOLCHAIN_BUILD := $(abspath $(TOOLCHAIN)/../../target/wiiu-toolchain)
BUILD ?= target
TOOLCHAIN_LIB := $(TOOLCHAIN_BUILD)/lib
WUT := $(TOOLCHAIN_BUILD)/src/wut
ELF2RPL := $(TOOLCHAIN_BUILD)/bin/elf2rpl
LATTE_ASSEMBLER := $(TOOLCHAIN_BUILD)/bin/latte-assembler
GTXCONV := $(TOOLCHAIN_BUILD)/bin/gtxconv
WUHBTOOL := $(TOOLCHAIN_BUILD)/bin/wuhbtool

# Apple's clang can't target PowerPC, so prefer Homebrew's LLVM when it's installed
LLVM := $(firstword $(wildcard /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/))
CC := $(LLVM)clang
LD := ld.lld

# The Wii U has a PowerPC 750 compatible Espresso CPU, Cafe OS provides the system libraries.
# All code is optimized for size, like the Rust examples (opt-level = "s"). Espresso has the
# paired singles of the Wii's Gekko compatible CPU, so GEKKO makes gu.h declare the ps_ functions.
CFLAGS := --target=powerpc-none-eabi -mcpu=750 -ffreestanding -nostdlibinc \
	-ffunction-sections -fdata-sections -DESPRESSO -DGEKKO -D__WIIU__ -D__WUT__ -D_DEFAULT_SOURCE -Os -g -std=c23 \
	-Wall -Wextra -I$(TOOLCHAIN_BUILD)/sysroot/include -I$(WUT)/include -I$(WUT)/libraries/libwhb/include \
	-I$(WUT)/libraries/libgfd/include -I$(TOOLCHAIN_BUILD)/include

OBJECTS := $(patsubst src/%.c,$(BUILD)/%.o,$(wildcard src/*.c))
LIBRARIES := $(TOOLCHAIN_LIB)/libwut.a $(TOOLCHAIN_LIB)/libgu.a $(TOOLCHAIN_BUILD)/sysroot/lib/libc.a $(TOOLCHAIN_LIB)/libcompiler-rt.a

$(BUILD)/$(NAME).rpx: $(BUILD)/$(NAME).elf $(ELF2RPL)
	$(ELF2RPL) $< $@ > /dev/null

# elf2rpl needs the relocations to make the imports of the system libraries
$(BUILD)/$(NAME).elf: $(OBJECTS) $(LIBRARIES) $(TOOLCHAIN)/wiiu.ld $(TOOLCHAIN_LIB)/imports.ld
	$(LD) -T $(TOOLCHAIN)/wiiu.ld -L $(TOOLCHAIN_LIB) --gc-sections --emit-relocs -z nocopyreloc \
		$(OBJECTS) $(LIBRARIES) -o $@

# The sources embed the assembled shaders and the converted textures
SHADERS := $(if $(wildcard src/shader.vsh),$(BUILD)/shader.gsh)
TEXTURES := $(if $(wildcard src/assets/textures.toml),$(BUILD)/textures.gtx)
$(BUILD)/%.o: src/%.c $(SHADERS) $(TEXTURES) | $(TOOLCHAIN_LIB)/libwut.a
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MMD -MP -c $< -o $@

$(BUILD)/shader.gsh: src/shader.vsh src/shader.psh $(LATTE_ASSEMBLER)
	@mkdir -p $(dir $@)
	$(LATTE_ASSEMBLER) assemble --vsh=src/shader.vsh --psh=src/shader.psh $@

$(BUILD)/$(NAME).wuhb: $(BUILD)/$(NAME).rpx $(wildcard meta/*) $(WUHBTOOL)
	$(TOOLCHAIN)/bin/bundle $< meta $@

$(BUILD)/textures.gtx: src/assets/textures.toml $(filter-out %.toml,$(wildcard src/assets/*)) $(GTXCONV)
	@mkdir -p $(dir $@)
	$(GTXCONV) -c $< -o $@

# The toolchain script knows itself when it needs to rebuild, one run builds all libraries
$(TOOLCHAIN_LIB)/libwut.a: FORCE
	@$(TOOLCHAIN)/build.sh
$(TOOLCHAIN_LIB)/libgu.a $(TOOLCHAIN_BUILD)/sysroot/lib/libc.a $(TOOLCHAIN_LIB)/libcompiler-rt.a $(TOOLCHAIN_LIB)/imports.ld \
	$(ELF2RPL) $(LATTE_ASSEMBLER) $(GTXCONV) $(WUHBTOOL): $(TOOLCHAIN_LIB)/libwut.a ;

run: $(BUILD)/$(NAME).elf
	$(TOOLCHAIN)/bin/run $<

bundle: $(BUILD)/$(NAME).wuhb

clean:
	rm -rf $(BUILD)

.PHONY: run bundle clean FORCE

-include $(OBJECTS:.o=.d)
