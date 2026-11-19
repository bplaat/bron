NAME := test
BUILD := target
include ../../wii.mk

# Convert the test image to a TPL texture file with gxtexconv
$(BUILD)/test.tpl: src/assets/test.scf src/assets/test.png | $(TOOLCHAIN_LIB)/libogc.a
	@mkdir -p $(BUILD)
	cd $(BUILD) && $(abspath $(GXTEXCONV)) -s $(abspath src/assets/test.scf) -o test.tpl

$(BUILD)/main.o: $(BUILD)/test.tpl
