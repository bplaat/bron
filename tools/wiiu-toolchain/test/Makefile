NAME := test
BUILD := ../../../target/wiiu-toolchain/test
include ../wiiu.mk

$(BUILD)/fixtures.gtx: ../gtxconv/tests/fixtures/textures.toml ../gtxconv/tests/fixtures/texture.png $(GTXCONV)
	@mkdir -p $(BUILD)
	$(GTXCONV) -c $< -o $@

$(BUILD)/main.o: $(BUILD)/fixtures.gtx

# Exercise the same Latte shader assembly and GFD relocation as the textured cube example.
$(BUILD)/fixture.gsh: ../../../examples/wiiu/cube/c/src/shader.vsh ../../../examples/wiiu/cube/c/src/shader.psh $(LATTE_ASSEMBLER)
	@mkdir -p $(BUILD)
	$(LATTE_ASSEMBLER) assemble --vsh=$< --psh=$(word 2,$^) $@

$(BUILD)/main.o: $(BUILD)/fixture.gsh
