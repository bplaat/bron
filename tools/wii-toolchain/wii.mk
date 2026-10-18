# Make rules to build a Wii program with clang and lld, include this from an example Makefile:
#
#   NAME := triangles
#   include ../../../../tools/wii-toolchain/wii.mk
#
# Then `make` builds $(BUILD)/$(NAME).dol from src/*.c and `make run` starts it in Dolphin.

TOOLCHAIN := $(patsubst %/,%,$(dir $(lastword $(MAKEFILE_LIST))))
TOOLCHAIN_BUILD := $(abspath $(TOOLCHAIN)/../../target/wii-toolchain)
BUILD ?= target
LIBOGC := $(TOOLCHAIN_BUILD)/libogc
ELF2DOL := $(TOOLCHAIN_BUILD)/elf2dol
GXTEXCONV := $(TOOLCHAIN_BUILD)/gxtexconv

# Apple's clang can't target PowerPC, so prefer Homebrew's LLVM when it's installed
LLVM := $(firstword $(wildcard /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/))
CC := $(LLVM)clang
LD := ld.lld

# The Wii has a PowerPC 750 CPU and there is no OS, so no standard library from the host.
# All code is optimized for size, like the Rust examples (opt-level = "s").
CFLAGS := --target=powerpc-none-eabi -mcpu=750 -ffreestanding -nostdlibinc \
	-ffunction-sections -fdata-sections -DGEKKO -DHW_RVL -D_DEFAULT_SOURCE -Os -g -std=c23 -Wall -Wextra \
	-I$(TOOLCHAIN_BUILD)/sysroot/include -I$(LIBOGC)/gc -I$(LIBOGC)/gc/ogc -I$(LIBOGC)/gc/ogc/machine \
	-I$(LIBOGC)/gc/bte -I$(LIBOGC)/gc/wiiuse

OBJECTS := $(patsubst src/%.c,$(BUILD)/%.o,$(wildcard src/*.c))
LIBRARIES := $(addprefix $(TOOLCHAIN_BUILD)/,libogc.a sysroot/lib/libc.a libcompiler-rt.a)

$(BUILD)/$(NAME).dol: $(BUILD)/$(NAME).elf
	$(ELF2DOL) $< $@

$(BUILD)/$(NAME).elf: $(OBJECTS) $(LIBRARIES)
	$(LD) -T $(TOOLCHAIN)/wii.ld --gc-sections $^ -o $@

$(BUILD)/%.o: src/%.c | $(TOOLCHAIN_BUILD)/libogc.a
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MMD -MP -c $< -o $@

# The toolchain script knows itself when it needs to rebuild, one run builds all libraries
$(TOOLCHAIN_BUILD)/libogc.a: FORCE
	@$(TOOLCHAIN)/build.sh
$(TOOLCHAIN_BUILD)/sysroot/lib/libc.a $(TOOLCHAIN_BUILD)/libcompiler-rt.a: $(TOOLCHAIN_BUILD)/libogc.a ;

run: $(BUILD)/$(NAME).elf
	$(TOOLCHAIN)/run.sh $<

clean:
	rm -rf $(BUILD)

.PHONY: run clean FORCE

-include $(OBJECTS:.o=.d)
