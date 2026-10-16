#!/usr/bin/env bash
# Builds DOOM for the Nintendo DS: fetches doomgeneric, applies patches/ and
# builds target/doom.nds with the WADs from assets/ embedded.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   dev             fetch and patch, then edit and commit in target/<workdir>/
#   export-patches  write the commits in target/<workdir>/ back to patches/
#   clean           remove target/
set -euo pipefail
cd "$(dirname "$0")"

version='dcb7a8dbc7a16ce3dda29382ac9aae9d77d21284'
source_url="https://github.com/ozkl/doomgeneric/archive/${version}.tar.gz"
source_sha256='1bd3f7f26220494159a38d71f2847ec81b58d6bbd7c7c8d81b08993018001148'

# The freely redistributable DOOM shareware v1.9 (doom1.wad) is embedded by
# default, from Debian's doom-wad-shareware source package on archive.org
shareware_url='https://archive.org/download/doom-wad-shareware_1.9_11apr2025-mirror/doom-wad-shareware_1.9.fixed.orig.tar.gz'
shareware_sha256='e02c8b5e01be7373d4c53f82556118e2aaaf8f83fa2af5eee1efadf9c55c4eb1'
doom1_sha256='1d7d43be501e67d927e415e0b8f3e29c3bf33075e859721816f652a526cac771'

PORT_DIR="$PWD"
BUILD_DIR="$PORT_DIR/target"
ASSETS_DIR="$PORT_DIR/assets"
SRC_DIR="$BUILD_DIR/doomgeneric-$version"

TOOLCHAIN_DIR="$(cd ../toolchain && pwd)"

msg() { printf '\033[1;34m[doom]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[doom]\033[0m %s\n' "$*" >&2; exit 1; }
git_src() { git -C "$SRC_DIR" "$@"; }

# Downloads url to dest (unless present) and verifies its sha256
download() {
    local url="$1" sha="$2" dest="$3"
    if [ ! -f "$dest" ]; then
        msg "Downloading $url"
        mkdir -p "$(dirname "$dest")"
        curl -fL --retry 3 -o "$dest.part" "$url"
        mv "$dest.part" "$dest"
    fi
    echo "$sha  $dest" | shasum -a 256 -c --status || { rm -f "$dest"; die "Checksum mismatch for $url"; }
}

# Extracts the given files (lowercase, under prefix in the installer) from a
# GOG setup_*.exe next to this script into assets/ with innoextract, fails
# without the installer or innoextract
gog_extract() {
    local prefix="$1" setup dir="$BUILD_DIR/gog" file includes=()
    shift
    setup="$(compgen -G "$PORT_DIR/setup_*.exe" | head -n 1)" || return 1
    command -v innoextract > /dev/null || { msg "Install innoextract to use $(basename "$setup")"; return 1; }
    for file in "$@"; do includes+=(-I "/$prefix$file"); done
    msg "Extracting $(basename "$setup")"
    rm -rf "$dir"
    innoextract -s -L -d "$dir" "${includes[@]}" "$setup" || { rm -rf "$dir"; msg "Failed to extract $(basename "$setup")"; return 1; }
    for file in "$@"; do
        [ -f "$dir/$prefix$file" ] || { msg "No $file in $(basename "$setup")"; rm -rf "$dir"; return 1; }
    done
    for file in "$@"; do
        mkdir -p "$(dirname "$ASSETS_DIR/$file")"
        mv "$dir/$prefix$file" "$ASSETS_DIR/$file"
    done
    rm -rf "$dir"
}

fetch() {
    local archive
    archive="$BUILD_DIR/$(basename "$source_url")"
    download "$source_url" "$source_sha256" "$archive"
    if [ ! -d "$SRC_DIR" ]; then
        msg "Extracting $(basename "$archive")"
        tar -xf "$archive" -C "$BUILD_DIR"
    fi

    [ -f "$ASSETS_DIR/doom.wad" ] && return
    gog_extract '' doom.wad && { msg "Added The Ultimate DOOM doom.wad to assets/"; return; }
    [ -f "$ASSETS_DIR/doom1.wad" ] && return
    archive="$BUILD_DIR/$(basename "$shareware_url")"
    download "$shareware_url" "$shareware_sha256" "$archive"
    mkdir -p "$ASSETS_DIR"
    tar -xOf "$archive" doom-wad-shareware-1.9.fixed/doom1.wad > "$ASSETS_DIR/doom1.wad.part"
    echo "$doom1_sha256  $ASSETS_DIR/doom1.wad.part" | shasum -a 256 -c --status || die "Unexpected doom1.wad"
    mv "$ASSETS_DIR/doom1.wad.part" "$ASSETS_DIR/doom1.wad"
    msg "Added the DOOM shareware doom1.wad to assets/"
}

# Turns the pristine source into a git repo and applies the patches as commits
patch() {
    [ -d "$SRC_DIR/.git" ] && return
    msg "Applying patches"
    git_src init -q
    # Deterministic identity so patch commits are reproducible
    git_src config user.name "Bastiaan van der Plaat"
    git_src config user.email bastiaan.v.d.plaat@gmail.com
    git_src config commit.gpgsign false
    git_src add -A -f
    git_src commit -q -m "Upstream $version"
    git_src tag upstream
    git_src am -q --keep-cr "$PORT_DIR"/patches/*.patch
}

# Writes the commits on top of upstream back to patches/
export_patches() {
    [ -d "$SRC_DIR/.git" ] || die "No patched source tree, run ./build.sh dev first"
    rm -f "$PORT_DIR"/patches/*.patch
    git_src format-patch -q --no-numbered --zero-commit --no-signature -o "$PORT_DIR/patches" upstream..HEAD
    msg "Exported $(git_src rev-list --count upstream..HEAD) patches"
}

build() {
    "$TOOLCHAIN_DIR/build.sh"

    # WADs in assets/ are embedded into the ROM through NitroFS
    local nitro="$BUILD_DIR/nitrofs" wad
    rm -rf "$nitro"
    mkdir -p "$nitro"
    for wad in "$ASSETS_DIR"/*.[wW][aA][dD]; do
        [ -f "$wad" ] && cp "$wad" "$nitro/$(basename "$wad" | tr '[:upper:]' '[:lower:]')"
    done
    # The full game has the shareware episode too
    [ -f "$nitro/doom.wad" ] && rm -f "$nitro/doom1.wad"
    msg "Embedding $(cd "$nitro" && echo *)"
    make -C "$SRC_DIR/doomgeneric" -f Makefile.nds -j"$(getconf _NPROCESSORS_ONLN)" \
        TOOLCHAIN="$TOOLCHAIN_DIR" ICON="$PORT_DIR/icon.bmp" NITRO_DIR="$nitro"
    cp "$SRC_DIR/doomgeneric/doom.nds" "$BUILD_DIR/doom.nds"
    msg "Built target/doom.nds"
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) mkdir -p "$BUILD_DIR"; fetch; patch; build ;;
        dev) mkdir -p "$BUILD_DIR"; fetch; patch; msg "Edit and commit in target/doomgeneric-$version/, then run ./build.sh export-patches" ;;
        export-patches) export_patches ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
