BINARIES := redsquare-x86.exe redsquare-x64.exe
DEPS := redsquare.asm ../libwindows.inc redsquare.manifest icon.ico bassiebas.bmp Makefile

.PHONY: all clean
.DELETE_ON_ERROR:

all: $(BINARIES)

redsquare-x86.exe: $(DEPS)
	nasm -f bin $< -o $@

redsquare-x64.exe: $(DEPS)
	nasm -DWIN64 -f bin $< -o $@

clean:
	rm -f $(BINARIES)
