#!/usr/bin/env bash
# Builds Quake II for the Nintendo DS: fetches id Software's GPL Quake II
# source, applies patches/ and builds target/quake2.nds with the game data in
# assets/ embedded.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   dev             fetch and patch, then edit and commit in target/<workdir>/
#   export-patches  write the commits in target/<workdir>/ back to patches/
#   clean           remove target/
set -euo pipefail
cd "$(dirname "$0")"

version='372afde46e7defc9dd2d719a1732b8ace1fa096e'
source_url="https://github.com/id-Software/Quake-2/archive/${version}.tar.gz"
source_sha256='465bd8bc40a9f0a2899aac3baa045f1c3449788283cf9e8a1b800f9b3ff3fa05'

# The freely distributable Quake II demo 3.14 (baseq2/pak0.pak, the first unit)
# is embedded by default with its license, which has to go with it
demo_url='https://deponie.yamagi.org/quake2/idstuff/q2-314-demo-x86.exe'
demo_sha256='7ace5a43983f10d6bdc9d9b6e17a1032ba6223118d389bd170df89b945a04a1e'
pak0_sha256='cae257182f34d3913f3d663e1d7cf865d668feda6af393d4ecf3e9e408b48d09'

PORT_DIR="$PWD"
BUILD_DIR="$PORT_DIR/target"
ASSETS_DIR="$PORT_DIR/assets"
SRC_DIR="$BUILD_DIR/Quake-2-$version"

TOOLCHAIN_DIR="$(cd ../../../tools/nds-toolchain && pwd)"

msg() { printf '\033[1;34m[quake2]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[quake2]\033[0m %s\n' "$*" >&2; exit 1; }
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

    # The full game's baseq2/pak0.pak replaces the demo one, with its license
    [ -f "$ASSETS_DIR/baseq2/pak1.pak" ] && return
    if gog_extract app/ baseq2/pak{0,1,2}.pak; then
        rm -f "$ASSETS_DIR/baseq2/license.txt"
        msg "Added the Quake II baseq2/pak0.pak, pak1.pak and pak2.pak to assets/"
        return
    fi
    [ -f "$ASSETS_DIR/baseq2/pak0.pak" ] && return
    archive="$BUILD_DIR/$(basename "$demo_url")"
    download "$demo_url" "$demo_sha256" "$archive"
    mkdir -p "$ASSETS_DIR/baseq2"
    unzip -p "$archive" Install/Data/DOCS/license.txt > "$ASSETS_DIR/baseq2/license.txt"
    unzip -p "$archive" Install/Data/baseq2/pak0.pak > "$ASSETS_DIR/baseq2/pak0.pak.part"
    echo "$pak0_sha256  $ASSETS_DIR/baseq2/pak0.pak.part" | shasum -a 256 -c --status || die "Unexpected pak0.pak"
    mv "$ASSETS_DIR/baseq2/pak0.pak.part" "$ASSETS_DIR/baseq2/pak0.pak"
    msg "Added the Quake II demo baseq2/pak0.pak to assets/"
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
    if compgen -G "$PORT_DIR/patches/*.patch" > /dev/null; then
        git_src am -q --keep-cr "$PORT_DIR"/patches/*.patch
    fi
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

    # assets/ is Quake II's base directory (baseq2/, mod directories) and is
    # embedded into the ROM through NitroFS, with lowercase names like Quake II uses
    local nitro="$BUILD_DIR/nitrofs" file dest
    rm -rf "$nitro"
    mkdir -p "$nitro"
    while IFS= read -r file; do
        dest="$nitro/$(printf '%s' "$file" | tr '[:upper:]' '[:lower:]')"
        mkdir -p "$(dirname "$dest")"
        cp "$ASSETS_DIR/$file" "$dest"
    done < <(cd "$ASSETS_DIR" && find . -type f ! -name '.*' | sed 's|^\./||')
    msg "Embedding $(cd "$nitro" && find . -type f | sed 's|^\./||' | sort | xargs)"
    make -C "$SRC_DIR" -f Makefile.nds -j"$(getconf _NPROCESSORS_ONLN)" \
        TOOLCHAIN="$TOOLCHAIN_DIR" ICON="$PORT_DIR/icon.bmp" NITRO_DIR="$nitro"
    cp "$SRC_DIR/quake2.nds" "$BUILD_DIR/quake2.nds"
    msg "Built target/quake2.nds"
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) mkdir -p "$BUILD_DIR"; fetch; patch; build ;;
        dev) mkdir -p "$BUILD_DIR"; fetch; patch; msg "Edit and commit in target/Quake-2-$version/, then run ./build.sh export-patches" ;;
        export-patches) export_patches ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
