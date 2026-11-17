# Cross-compile bastiaan.exe with MinGW-w64 (i386), images are generated with ImageMagick
CC      := i686-w64-mingw32-gcc
WINDRES := i686-w64-mingw32-windres
CFLAGS  := -O2 -s -Wall -Wextra -std=gnu11
LDFLAGS := -municode -mwindows -Wl,--no-insert-timestamp

# Order must match the ICON_* enum in bastiaan.c
ICONS := bassiemusic strepen bible coinlist redsquare warquest \
         blog github codepen linkedin facebook instagram reddit x youtube soundcloud

all: bastiaan.exe

bastiaan.exe: bastiaan.c bastiaan.o
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $^ -lgdi32 -lmsimg32 -lshell32 -lole32

bastiaan.o: bastiaan.rc bastiaan.manifest bastiaan.ico atlas.bmp
	$(WINDRES) -O coff -o $@ $<

# Uncompressed 32-bit BMP entries only, Windows XP can't read PNG icon entries
bastiaan.ico: bassiebas.jpg
	convert $< -alpha set \( -clone 0 -resize 48x48 \) \( -clone 0 -resize 32x32 \) \
		\( -clone 0 -resize 16x16 \) -delete 0 $@

# The SVG coder is missing, so convert the paths to MVG, render 8x and box downscale
icons/%.bmp: icons/%.svg
	awk 'match($$0, /viewBox="0 0 [0-9.]+/) { s = 192 / substr($$0, RSTART + 13, RLENGTH - 13); \
		printf "viewbox 0 0 192 192\nfill-rule nonzero\nfill black\naffine %g 0 0 %g 0 0\n", s, s } \
		match($$0, / d="[^"]+"/) { printf "path '\''%s'\''\n", substr($$0, RSTART + 4, RLENGTH - 5) }' $< | \
		convert -background white mvg:- -filter box -resize 24x24 -type TrueColor BMP3:$@

# Atlas: 48x48 avatar (pure white nudged so it stays opaque for TransparentBlt), then the 24x24 icons
atlas.bmp: avatar.png $(ICONS:%=icons/%.bmp)
	convert \( $< -background white -flatten -fill '#fefefe' -opaque white \) \
		$(ICONS:%=icons/%.bmp) -background white +append -type TrueColor BMP3:$@

clean:
	rm -f bastiaan.exe bastiaan.o bastiaan.ico atlas.bmp icons/*.bmp

.PHONY: all clean
