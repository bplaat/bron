#!/usr/bin/env bash
# Builds Wolfenstein 3D for the Nintendo DS: fetches Wolf4SDL, applies
# patches/ and builds target/wolf3d.nds with the game data in assets/ embedded.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   dev             fetch and patch, then edit and commit in target/<workdir>/
#   export-patches  write the commits in target/<workdir>/ back to patches/
#   clean           remove target/
set -euo pipefail
cd "$(dirname "$0")"

version='dc8b250af35fb0ace68db5eb879490b50068c20e'
source_url="https://github.com/KS-Presto/Wolf4SDL/archive/${version}.tar.gz"
source_sha256='aa83fbc1aa862cf646b7887019413c7163bbe26ace97096885366a6c7f9613b9'

# The freely redistributable Wolfenstein 3D shareware v1.4 (*.wl1, episode 1)
# is embedded by default, from an archive.org copy of its game data
shareware_url='https://archive.org/download/wolf3dsw/wolf3dsw.zip'
shareware_sha256='76ee5e73e7d6341aefff620989bb5f828e9d295982afd5415b62dee7fe54eb64'
shareware_files='
39351624ae6f8eef4b873e060c1a6f3e5ee7e81c4939c485275a89b145336338  audiohed.wl1
1e2c9ae30398a14c61a4ddd39aabaa0dbcc984cc4924a3e51df75574c259cfb9  audiot.wl1
a6a6654b342f2c027bcb22bfce0a41f9fc0063b775e9e4da0c771970e53e11aa  gamemaps.wl1
3458f661c9b875bca99ea22a7267771fad8f2a33c699ea732cf7c3322909bf8c  maphead.wl1
59878fec65f033b00dbb1240317d1793f5858213dc8013b4d68f9ac8b45b0c80  vgadict.wl1
d5176f843c53415132db199c19f38591eaf3cedd35a4f7eb2698c1865d83030d  vgagraph.wl1
f4cc800dc8444373092d4eaa5d6ab59d63d23a510a9d38b73ca9b3dbb700d18b  vgahead.wl1
698f217257e2cbb951a4d110ba09140291f38d0121b3784d1d6be59c03a6b47b  vswap.wl1'

PORT_DIR="$PWD"
BUILD_DIR="$PORT_DIR/target"
ASSETS_DIR="$PORT_DIR/assets"
SRC_DIR="$BUILD_DIR/Wolf4SDL-$version"

TOOLCHAIN_DIR="$(cd ../toolchain && pwd)"

msg() { printf '\033[1;34m[wolf3d]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[wolf3d]\033[0m %s\n' "$*" >&2; exit 1; }
git_src() { git -C "$SRC_DIR" "$@"; }
patch_checksum() { (cd "$PORT_DIR" && shasum -a 256 patches/*.patch); }

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
    local archive sha file missing=0
    archive="$BUILD_DIR/$(basename "$source_url")"
    download "$source_url" "$source_sha256" "$archive"
    if [ ! -d "$SRC_DIR" ]; then
        msg "Extracting $(basename "$archive")"
        tar -xf "$archive" -C "$BUILD_DIR"
    fi

    [ -f "$ASSETS_DIR/vswap.wl6" ] && return
    if gog_extract '' {audiohed,audiot,gamemaps,maphead,vgadict,vgagraph,vgahead,vswap}.wl6; then
        msg "Added the Wolfenstein 3D *.wl6 to assets/"
        return
    fi
    while read -r sha file; do
        [ -n "$file" ] || continue
        [ -f "$ASSETS_DIR/$file" ] || missing=1
    done <<< "$shareware_files"
    [ "$missing" -eq 0 ] && return
    archive="$BUILD_DIR/$(basename "$shareware_url")"
    download "$shareware_url" "$shareware_sha256" "$archive"
    mkdir -p "$ASSETS_DIR"
    while read -r sha file; do
        [ -n "$file" ] || continue
        [ -f "$ASSETS_DIR/$file" ] && continue
        unzip -p "$archive" "$(printf '%s' "$file" | tr '[:lower:]' '[:upper:]')" > "$ASSETS_DIR/$file.part"
        echo "$sha  $ASSETS_DIR/$file.part" | shasum -a 256 -c --status || die "Unexpected $file"
        mv "$ASSETS_DIR/$file.part" "$ASSETS_DIR/$file"
    done <<< "$shareware_files"
    msg "Added the Wolfenstein 3D shareware *.wl1 to assets/"
}

# Turns the pristine source into a git repo and applies the patches as commits
patch() {
    local checksum
    checksum="$(patch_checksum)"
    if [ -d "$SRC_DIR/.git" ]; then
        [ -f "$BUILD_DIR/patches.sha256" ] && [ "$(cat "$BUILD_DIR/patches.sha256")" = "$checksum" ] \
            || die "Patched sources are outdated or unverified. Run ./build.sh clean then ./build.sh; clean removes development edits in target/."
        return
    fi
    msg "Applying patches"
    git_src init -q
    # Deterministic identity so patch commits are reproducible
    git_src config user.name "Bastiaan van der Plaat"
    git_src config user.email bastiaan.v.d.plaat@gmail.com
    git_src config commit.gpgsign false
    git_src add -A -f
    git_src commit -q -m "Upstream $version"
    git_src tag upstream
    if compgen -G "$PORT_DIR/patches/*.patch" > /dev/null; then
        git_src am -q --keep-cr "$PORT_DIR"/patches/*.patch
    fi
    printf '%s\n' "$checksum" > "$BUILD_DIR/patches.sha256"
}

# Writes the commits on top of upstream back to patches/
export_patches() {
    [ -d "$SRC_DIR/.git" ] || die "No patched source tree, run ./build.sh dev first"
    rm -f "$PORT_DIR"/patches/*.patch
    git_src format-patch -q --no-numbered --zero-commit --no-signature -o "$PORT_DIR/patches" upstream..HEAD
    patch_checksum > "$BUILD_DIR/patches.sha256"
    msg "Exported $(git_src rev-list --count upstream..HEAD) patches"
}

build() {
    "$TOOLCHAIN_DIR/build.sh"

    # The game data files in assets/ are embedded into the ROM through
    # NitroFS, with the lowercase names Wolf4SDL opens
    local nitro="$BUILD_DIR/nitrofs" file
    rm -rf "$nitro"
    mkdir -p "$nitro"
    for file in "$ASSETS_DIR"/*.*; do
        case "$(basename "$file" | tr '[:upper:]' '[:lower:]')" in
            *.wl[136]) cp "$file" "$nitro/$(basename "$file" | tr '[:upper:]' '[:lower:]')" ;;
        esac
    done
    # The full game has the shareware episode too
    compgen -G "$nitro/*.wl[36]" > /dev/null && rm -f "$nitro"/*.wl1
    msg "Embedding $(cd "$nitro" && echo *)"
    make -C "$SRC_DIR" -f Makefile.nds -j"$(getconf _NPROCESSORS_ONLN)" \
        TOOLCHAIN="$TOOLCHAIN_DIR" ICON="$PORT_DIR/icon.bmp" NITRO_DIR="$nitro"
    cp "$SRC_DIR/wolf3d.nds" "$BUILD_DIR/wolf3d.nds"
    msg "Built target/wolf3d.nds"
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) mkdir -p "$BUILD_DIR"; fetch; patch; build ;;
        dev) mkdir -p "$BUILD_DIR"; fetch; patch; msg "Edit and commit in target/Wolf4SDL-$version/, then run ./build.sh export-patches" ;;
        export-patches) export_patches ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
