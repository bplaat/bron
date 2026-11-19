# Make rules to build a Wii program with clang and lld, include this from an example Makefile:
#
#   NAME := triangles
#   include ../../../../tools/wii-toolchain/wii.mk
#
# Then `make` builds $(BUILD)/$(NAME).dol from src/*.c and `make run` starts it in Dolphin.

TOOLCHAIN := $(patsubst %/,%,$(dir $(lastword $(MAKEFILE_LIST))))
TOOLCHAIN_BUILD := $(abspath $(TOOLCHAIN)/../../target/wii-toolchain)
TOOLCHAIN_LIB := $(TOOLCHAIN_BUILD)/lib
BUILD ?= target
LIBOGC := $(TOOLCHAIN_BUILD)/src/libogc
ELF2DOL := $(TOOLCHAIN_BUILD)/bin/elf2dol
GXTEXCONV := $(TOOLCHAIN_BUILD)/bin/gxtexconv

# Apple's clang can't target PowerPC, so prefer Homebrew's LLVM when it's installed
LLVM := $(firstword $(wildcard /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/))
CC := $(LLVM)clang
LD := ld.lld

# The Wii has a PowerPC 750 CPU and there is no OS, so no standard library from the host.
# All code is optimized for size, like the Rust examples (opt-level = "s").
CFLAGS := --target=powerpc-none-eabi -mcpu=750 -ffreestanding -nostdlibinc \
	-ffunction-sections -fdata-sections -DGEKKO -DHW_RVL -D_DEFAULT_SOURCE -Os -g -std=c23 -Wall -Wextra \
	-I$(TOOLCHAIN_BUILD)/src/libdvm/include -I$(TOOLCHAIN_BUILD)/sysroot/include -I$(LIBOGC)/gc -I$(LIBOGC)/gc/ogc -I$(LIBOGC)/gc/ogc/machine \
	-I$(LIBOGC)/gc/bte -I$(LIBOGC)/gc/wiiuse

OBJECTS := $(patsubst src/%.c,$(BUILD)/%.o,$(wildcard src/*.c))
LIBRARIES := $(TOOLCHAIN_LIB)/libogc.a $(TOOLCHAIN_LIB)/libdvm.a $(TOOLCHAIN_BUILD)/sysroot/lib/libc.a \
	$(TOOLCHAIN_LIB)/libcompiler-rt.a

$(BUILD)/$(NAME).dol: $(BUILD)/$(NAME).elf $(ELF2DOL)
	$(ELF2DOL) $< $@

$(BUILD)/$(NAME).elf: $(OBJECTS) $(LIBRARIES) $(TOOLCHAIN)/wii.ld
	$(LD) -T $(TOOLCHAIN)/wii.ld --gc-sections $(OBJECTS) $(LIBRARIES) -o $@

$(BUILD)/%.o: src/%.c | $(TOOLCHAIN_LIB)/libogc.a
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MMD -MP -c $< -o $@

# The toolchain script knows itself when it needs to rebuild, one run builds all libraries
$(TOOLCHAIN_LIB)/libogc.a: FORCE
	@$(TOOLCHAIN)/build.sh
$(TOOLCHAIN_LIB)/libdvm.a $(TOOLCHAIN_BUILD)/sysroot/lib/libc.a $(TOOLCHAIN_LIB)/libcompiler-rt.a \
	$(ELF2DOL) $(GXTEXCONV): $(TOOLCHAIN_LIB)/libogc.a ;

run: $(BUILD)/$(NAME).elf
	$(TOOLCHAIN)/bin/run $<

clean:
	rm -rf $(BUILD)

.PHONY: run clean FORCE

-include $(OBJECTS:.o=.d)
