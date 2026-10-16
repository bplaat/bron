#!/usr/bin/env bash
# Builds Quake for the Nintendo DS: fetches id Software's GPL Quake source,
# applies patches/ and builds target/quake.nds with the game data in assets/
# embedded.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   dev             fetch and patch, then edit and commit in target/<workdir>/
#   export-patches  write the commits in target/<workdir>/ back to patches/
#   clean           remove target/
set -euo pipefail
cd "$(dirname "$0")"

version='bf4ac424ce754894ac8f1dae6a3981954bc9852d'
source_url="https://github.com/id-Software/Quake/archive/${version}.tar.gz"
source_sha256='fb98e8c99740123f3fac830c1387584ec1f684db8908cbf42b7187d06d399013'

# The freely redistributable Quake shareware v1.06 (id1/pak0.pak, episode 1)
# is embedded by default, from an installed copy on archive.org
shareware_url='https://archive.org/download/msdos_Quake106_shareware/msdos_Quake106_shareware.zip'
shareware_sha256='920f4609801d0bdbea5b6738cec49da846df4ff8ce0d46c901ea080dd4437833'
pak0_sha256='35a9c55e5e5a284a159ad2a62e0e8def23d829561fe2f54eb402dbc0a9a946af'

PORT_DIR="$PWD"
BUILD_DIR="$PORT_DIR/target"
ASSETS_DIR="$PORT_DIR/assets"
SRC_DIR="$BUILD_DIR/Quake-$version"

TOOLCHAIN_DIR="$(cd ../toolchain && pwd)"

msg() { printf '\033[1;34m[quake]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[quake]\033[0m %s\n' "$*" >&2; exit 1; }
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

    # The full game's id1/pak0.pak replaces the shareware one
    [ -f "$ASSETS_DIR/id1/pak1.pak" ] && return
    gog_extract app/ id1/pak0.pak id1/pak1.pak && { msg "Added the Quake id1/pak0.pak and id1/pak1.pak to assets/"; return; }
    [ -f "$ASSETS_DIR/id1/pak0.pak" ] && return
    archive="$BUILD_DIR/$(basename "$shareware_url")"
    download "$shareware_url" "$shareware_sha256" "$archive"
    mkdir -p "$ASSETS_DIR/id1"
    unzip -p "$archive" ID1/PAK0.PAK > "$ASSETS_DIR/id1/pak0.pak.part"
    echo "$pak0_sha256  $ASSETS_DIR/id1/pak0.pak.part" | shasum -a 256 -c --status || die "Unexpected pak0.pak"
    mv "$ASSETS_DIR/id1/pak0.pak.part" "$ASSETS_DIR/id1/pak0.pak"
    msg "Added the Quake shareware id1/pak0.pak to assets/"
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

    # assets/ is Quake's base directory (id1/, mod directories) and is
    # embedded into the ROM through NitroFS, with lowercase names like Quake uses
    local nitro="$BUILD_DIR/nitrofs" file dest
    rm -rf "$nitro"
    mkdir -p "$nitro"
    while IFS= read -r file; do
        dest="$nitro/$(printf '%s' "$file" | tr '[:upper:]' '[:lower:]')"
        mkdir -p "$(dirname "$dest")"
        cp "$ASSETS_DIR/$file" "$dest"
    done < <(cd "$ASSETS_DIR" && find . -type f ! -name '.*' | sed 's|^\./||')
    msg "Embedding $(cd "$nitro" && find . -type f | sed 's|^\./||' | sort | xargs)"
    make -C "$SRC_DIR/WinQuake" -f Makefile.nds -j"$(getconf _NPROCESSORS_ONLN)" \
        TOOLCHAIN="$TOOLCHAIN_DIR" ICON="$PORT_DIR/icon.bmp" NITRO_DIR="$nitro"
    cp "$SRC_DIR/WinQuake/quake.nds" "$BUILD_DIR/quake.nds"
    msg "Built target/quake.nds"
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) mkdir -p "$BUILD_DIR"; fetch; patch; build ;;
        dev) mkdir -p "$BUILD_DIR"; fetch; patch; msg "Edit and commit in target/Quake-$version/, then run ./build.sh export-patches" ;;
        export-patches) export_patches ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
