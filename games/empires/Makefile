CC ?= cc
PKG_CONFIG ?= pkg-config
CFLAGS ?= -O2
CFLAGS += -std=c11 -Wall -Wextra
CPPFLAGS += $(shell $(PKG_CONFIG) --cflags sdl2)
LDLIBS += $(shell $(PKG_CONFIG) --libs sdl2) -lm

PROGRAMS := target/empires target/drsviewer

.PHONY: all clean
all: $(PROGRAMS)

target/%: %.c $(wildcard *.h) | target
	$(CC) $(CPPFLAGS) $(CFLAGS) $< $(LDFLAGS) $(LDLIBS) -o $@

target:
	mkdir -p $@

clean:
	rm -rf target
