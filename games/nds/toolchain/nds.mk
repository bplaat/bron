# Make rules to build a Nintendo DS program with clang and lld, include this from a port's
# Makefile after setting:
#
#   NAME          builds $(NAME).elf and $(NAME).nds
#   SOURCES       the C and assembly files without extension, Thumb code in main RAM
#   ITCM_SOURCES  the same for ARM code in ITCM, the fast memory for hot code
#   ARM7_SOURCES  the same for ARM code on the ARM7 in its WRAM (optional), linked with the default
#                 ARM7 program as $(NAME)7.elf, which calls their arm7PortInit before its main loop
#   OFILES        other objects to link (optional)
#   GAME_TITLE, GAME_SUBTITLE1 and GAME_SUBTITLE2  the three lines of the title in the DS menu
#   HEADER_TITLE  the up to 12 character title in the header, GAME_TITLE by default
#   ICON          the 32x32 icon bitmap, calico's by default
#   NITRO_DIR     the directory embedded as NitroFS (optional)
#
# and add the port's flags to CFLAGS after it. The objects go to $(BUILD)/. The port build scripts
# run the toolchain build first and pass TOOLCHAIN. `make run` starts the program in melonDS.

TOOLCHAIN_TARGET := $(TOOLCHAIN)/target
BUILD ?= build
OFILES += $(addprefix $(BUILD)/,$(addsuffix .o,$(SOURCES)) $(addsuffix .itcm.o,$(ITCM_SOURCES)))

# Apple's clang can't target ARM bare metal, so prefer Homebrew's LLVM when it's installed
LLVM := $(firstword $(wildcard /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/))
CC := $(LLVM)clang
AR := $(LLVM)llvm-ar
LD := ld.lld

# The ARM9 is an ARM946E-S (ARMv5TE) without floating point unit, there is no OS so no standard
# library from the host. -O3 unrolls and inlines the renderers' loops, DOOM runs 10% faster. Calls
# to memcpy, memmove and memset stay calls to the libc's, which are VRAM safe: inlined, a copy to
# VRAM through a byte pointer writes bytes, which VRAM ignores
CFLAGS := --target=armv5te-none-eabi -mcpu=arm946e-s -mthumb -DARM9 -D__NDS__ -nostdlibinc \
	-ffunction-sections -fdata-sections -g -O3 -fno-builtin-memcpy -fno-builtin-memmove \
	-fno-builtin-memset -Wall -I$(TOOLCHAIN)/libc/include \
	-I$(TOOLCHAIN_TARGET)/calico/include -I$(TOOLCHAIN_TARGET)/libnds/include \
	-I$(TOOLCHAIN_TARGET)/libdvm/include
LIBS := $(addprefix $(TOOLCHAIN_TARGET)/,libnds9.a libdvm.a libcalico9.a libc9.a libcompiler-rt9.a)

# The ARM7 is an ARM7TDMI (ARMv4T), which can't switch between ARM and Thumb code in a direct call,
# so its calls are long calls through a register
ARM7_CFLAGS := --target=armv4t-none-eabi -mcpu=arm7tdmi -marm -mlong-calls -DARM7 -D__NDS__ \
	-nostdlibinc -ffunction-sections -fdata-sections -g -O2 -Wall -I$(TOOLCHAIN)/libc/include \
	-I$(TOOLCHAIN_TARGET)/calico/include
ARM7_LIBS := $(addprefix $(TOOLCHAIN_TARGET)/,libcalico7.a libc7.a libcompiler-rt7.a)
ARM7_OFILES := $(addprefix $(BUILD)/,$(addsuffix .arm7.o,$(ARM7_SOURCES)))
ARM7_ELF := $(if $(ARM7_SOURCES),$(NAME)7.elf,$(TOOLCHAIN_TARGET)/ds7.elf)

HEADER_TITLE ?= $(GAME_TITLE)
ICON ?= $(TOOLCHAIN_TARGET)/calico/share/nds-icon.bmp

# Homebrew has no game code
$(NAME).nds: $(NAME).elf $(ARM7_ELF) $(ICON) $(if $(NITRO_DIR),$(shell find $(NITRO_DIR) -type f))
	$(TOOLCHAIN_TARGET)/bin/ndstool -c $@ -9 $< -7 $(ARM7_ELF) -b $(ICON) \
		"$(GAME_TITLE);$(GAME_SUBTITLE1);$(GAME_SUBTITLE2)" -g "####" 00 "$(HEADER_TITLE)" \
		$(if $(NITRO_DIR),-d $(NITRO_DIR))

$(NAME).elf: $(OFILES) $(LIBS)
	$(LD) -T $(TOOLCHAIN_TARGET)/calico/share/ds9.ld --gc-sections --nmagic -Map $(NAME).map \
		$(OFILES) --start-group $(LIBS) --end-group -o $@

$(NAME)7.elf: $(TOOLCHAIN_TARGET)/objects/ds7/arm7_main.o $(ARM7_OFILES) $(ARM7_LIBS)
	$(LD) -T $(TOOLCHAIN_TARGET)/calico/share/ds7.ld --gc-sections --nmagic --section-start=.main=0x02ff0000 \
		$(TOOLCHAIN_TARGET)/objects/ds7/arm7_main.o $(ARM7_OFILES) $(ARM7_LIBS) -o $@

$(BUILD)/%.arm7.o: %.c
	@mkdir -p $(dir $@)
	$(CC) $(ARM7_CFLAGS) -MMD -MP -c $< -o $@

# The ITCM rules come first, make 3.81 takes the first pattern rule that matches
$(BUILD)/%.itcm.o: CFLAGS += -marm

$(BUILD)/%.itcm.o: %.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MMD -MP -c $< -o $@

$(BUILD)/%.itcm.o: %.s
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MMD -MP -x assembler-with-cpp -c $< -o $@

$(BUILD)/%.o: %.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MMD -MP -c $< -o $@

$(BUILD)/%.o: %.s
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -MMD -MP -x assembler-with-cpp -c $< -o $@

run: $(NAME).nds
	$(TOOLCHAIN)/run.sh $<

clean:
	rm -rf $(BUILD) $(NAME).elf $(NAME)7.elf $(NAME).map $(NAME).nds

.PHONY: run clean

-include $(OFILES:.o=.d) $(ARM7_OFILES:.o=.d)
