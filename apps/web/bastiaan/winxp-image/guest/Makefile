# Cross-compile the v86 guest integration with MinGW-w64 (i686)
CC       := i686-w64-mingw32-gcc
CXX      := i686-w64-mingw32-g++
DDK      := /usr/i686-w64-mingw32/include/ddk
CFLAGS   := -O2 -s -Wall -Wextra -std=gnu11
KCFLAGS  := $(CFLAGS) -ffreestanding -D_X86_ -fno-asynchronous-unwind-tables -I$(DDK)
KLDFLAGS := -nostdlib -shared -Wl,--subsystem,native:5.01 -Wl,--image-base,0x10000 \
            -Wl,--entry,_DriverEntry@8 -Wl,--file-alignment,0x200 -Wl,--section-alignment,0x1000 \
            -Wl,--disable-dynamicbase -Wl,--disable-nxcompat -Wl,--no-insert-timestamp \
            -Wl,--exclude-all-symbols
ULDFLAGS := -municode -mwindows -Wl,--no-insert-timestamp

all: v86mou.sys v86vid.sys v86agent.exe v86setup.exe activate/confid.exe user.bmp

v86mou.sys: v86mou.c v86.h
	$(CC) $(KCFLAGS) $(KLDFLAGS) -o $@ $< -lntoskrnl -lhal

v86vid.sys: v86vid.c v86.h
	$(CC) $(KCFLAGS) $(KLDFLAGS) -o $@ $< -lvideoprt

v86agent.exe: v86agent.c v86.h
	$(CC) $(CFLAGS) $(ULDFLAGS) -o $@ $< -lshell32 -lole32 -luuid -ladvapi32 -liphlpapi

v86setup.exe: v86setup.c
	$(CC) $(CFLAGS) $(ULDFLAGS) -o $@ $< -ladvapi32 -lnetapi32 -lsetupapi

activate/confid.exe: activate/main.cpp activate/confid.cpp activate/confid.h
	$(CXX) -O2 -s -std=c++17 -static -Wl,--no-insert-timestamp -o $@ activate/main.cpp activate/confid.cpp

# The account picture: the site's 48x48 avatar as a 24-bit BMP
user.bmp: ../../web/public/images/avatar-48x48.png
	convert $< -type TrueColor BMP3:$@

clean:
	rm -f v86mou.sys v86vid.sys v86agent.exe v86setup.exe activate/confid.exe user.bmp

.PHONY: all clean
