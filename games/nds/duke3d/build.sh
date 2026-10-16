#!/usr/bin/env bash
# Builds Duke Nukem 3D for the Nintendo DS: fetches Chocolate Duke3D,
# applies patches/ and builds target/duke3d.nds with the game data in assets/
# embedded.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   dev             fetch and patch, then edit and commit in target/<workdir>/
#   export-patches  write the commits in target/<workdir>/ back to patches/
#   clean           remove target/
set -euo pipefail
cd "$(dirname "$0")"

version='1cfa1909aa1f5a5ab159301044ca499673bc2283'
source_url="https://github.com/fabiensanglard/chocolate_duke3D/archive/${version}.tar.gz"
source_sha256='21ef372db2e9c835b628a85e9317b2c2ec9cee04e766eff6075a862c5e3c540e'

# The freely redistributable Duke Nukem 3D shareware v1.3D (episode 1) is
# embedded by default, from 3D Realms' original 3dduke13.zip on archive.org:
# DUKE3D.GRP, inside the self-extracting DN3DSW13.SHR
shareware_url='https://archive.org/download/3dduke13/3dduke13.zip'
shareware_sha256='c67efd179022bc6d9bde54f404c707cbcbdc15423c20be72e277bc2bdddf3d0e'
shareware_files=(
    'f943d0c2e2a0803a644a2107c81ea897dec87596d9dd1a6a432131ad6f5818d6 DUKE3D.GRP'
)

PORT_DIR="$PWD"
BUILD_DIR="$PORT_DIR/target"
ASSETS_DIR="$PORT_DIR/assets"
SRC_DIR="$BUILD_DIR/chocolate_duke3D-$version"

TOOLCHAIN_DIR="$(cd ../toolchain && pwd)"

msg() { printf '\033[1;34m[duke3d]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[duke3d]\033[0m %s\n' "$*" >&2; exit 1; }
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
    local archive="$BUILD_DIR/$(basename "$source_url")" grp="$ASSETS_DIR/duke3d.grp" entry sha file
    download "$source_url" "$source_sha256" "$archive"
    if [ ! -d "$SRC_DIR" ]; then
        msg "Extracting $(basename "$archive")"
        tar -xf "$archive" -C "$BUILD_DIR"
    fi

    # The full game's duke3d.grp replaces the shareware one
    if [ -f "$grp" ] && ! echo "${shareware_files[0]%% *}  $grp" | shasum -a 256 -c --status; then
        return
    fi
    gog_extract '' duke3d.grp && { msg "Added the Duke Nukem 3D Atomic Edition duke3d.grp to assets/"; return; }
    [ -f "$grp" ] && return
    archive="$BUILD_DIR/$(basename "$shareware_url")"
    download "$shareware_url" "$shareware_sha256" "$archive"
    unzip -p "$archive" DN3DSW13.SHR > "$BUILD_DIR/DN3DSW13.SHR"
    mkdir -p "$ASSETS_DIR"
    for entry in "${shareware_files[@]}"; do
        sha="${entry%% *}"
        file="${entry#* }"
        # Verified before it goes to assets/, which is never fetched again
        unzip -p "$BUILD_DIR/DN3DSW13.SHR" "$file" > "$BUILD_DIR/$file"
        echo "$sha  $BUILD_DIR/$file" | shasum -a 256 -c --status || die "Unexpected $file"
        mv "$BUILD_DIR/$file" "$ASSETS_DIR/$(echo "$file" | tr '[:upper:]' '[:lower:]')"
    done
    rm -f "$BUILD_DIR/DN3DSW13.SHR"
    msg "Added the Duke Nukem 3D shareware duke3d.grp to assets/"
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

    # The files in assets/ are embedded into the ROM through NitroFS, with
    # lowercase names
    local nitro="$BUILD_DIR/nitrofs" file
    rm -rf "$nitro"
    mkdir -p "$nitro"
    for file in "$ASSETS_DIR"/*; do
        [ -f "$file" ] && cp "$file" "$nitro/$(basename "$file" | tr '[:upper:]' '[:lower:]')"
    done
    msg "Embedding $(cd "$nitro" && echo *)"
    make -C "$SRC_DIR" -f Makefile.nds -j"$(getconf _NPROCESSORS_ONLN)" \
        TOOLCHAIN="$TOOLCHAIN_DIR" ICON="$PORT_DIR/icon.bmp" NITRO_DIR="$nitro"
    cp "$SRC_DIR/duke3d.nds" "$BUILD_DIR/duke3d.nds"
    msg "Built target/duke3d.nds"
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) mkdir -p "$BUILD_DIR"; fetch; patch; build ;;
        dev) mkdir -p "$BUILD_DIR"; fetch; patch; msg "Edit and commit in target/chocolate_duke3D-$version/, then run ./build.sh export-patches" ;;
        export-patches) export_patches ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
