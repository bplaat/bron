NAME := CatDog
APP := $(NAME).app
BINARY := $(APP)/Contents/MacOS/$(NAME)
MINIMUM_VERSION := 11.0

.PHONY: all clean run
.DELETE_ON_ERROR:

all: $(BINARY)

$(BINARY): $(wildcard src/* assets/*) Info.plist LICENSE.serenity
	mkdir -p "$(APP)/Contents/MacOS" "$(APP)/Contents/Resources"
	clang -fobjc-arc -mmacosx-version-min=$(MINIMUM_VERSION) -Wall -Wextra -Werror \
		src/main.m -framework Cocoa -o "$(BINARY)"
	cp assets/*.png LICENSE.serenity "$(APP)/Contents/Resources/"
	plutil -convert binary1 -o "$(APP)/Contents/Info.plist" Info.plist
	codesign --force --sign - --options runtime "$(APP)"

run: all
	open "$(APP)"

clean:
	rm -rf "$(APP)"
