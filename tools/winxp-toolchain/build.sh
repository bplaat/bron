#!/usr/bin/env bash
# Builds the Windows XP toolchain in the repository target/winxp-toolchain/: copies the std
# sources of the nightly Rust to src/rust-std/ and applies patches/rust-std/, fetches mingw-w64
# and LLVM's compiler-rt builtins to src/ and builds them with clang into sysroot/, the C runtime
# on XP's own msvcrt.dll.
# bin/cargo and bin/cc run this before they build.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   export-patches  write the commits in ../../target/winxp-toolchain/src/rust-std/ back to patches/rust-std/
#   test            build test/rust/ and run it in Windows XP in QEMU
#   clean           remove ../../target/winxp-toolchain/
set -euo pipefail
cd "$(dirname "$0")"

# The std sources come from this nightly's rust-src, they must match its compiler
rust_toolchain='nightly-2026-11-14'
mingw_url='https://github.com/mingw-w64/mingw-w64.git'
mingw_version='v14.0.0'
mingw_commit='9b3dd0125792fe94d16cacdc596dbd42fca1b369'
llvm_url='https://github.com/llvm/llvm-project.git'
llvm_version='llvmorg-23.1.3'
llvm_commit='0d261d1ca552c95a8f007e061c787ac7132fbcbc'

TOOLCHAIN_DIR="$PWD"
ROOT_DIR="$(cd ../.. && pwd)"
BUILD_DIR="$ROOT_DIR/target/winxp-toolchain"
SRC_DIR="$BUILD_DIR/src"
OBJECTS_DIR="$BUILD_DIR/build"
RUST_DIR="$SRC_DIR/rust-std"
MINGW_DIR="$SRC_DIR/mingw-w64"
LLVM_DIR="$SRC_DIR/llvm"
SYSROOT="$BUILD_DIR/sysroot"

. "$TOOLCHAIN_DIR/llvm.sh"
LLVM=$llvm

# Windows XP runs on a Pentium 4 or newer, everything is optimized for size like the Rust programs.
# clang finds the mingw-w64 headers and libraries in the sysroot and compiler-rt in the resource
# directory next to it.
XP_CFLAGS=(--target=i686-w64-mingw32 --sysroot="$SYSROOT" -resource-dir="$BUILD_DIR/clang"
    -march=pentium4 -Os)

msg() { printf '\033[1;34m[toolchain]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[toolchain]\033[0m %s\n' "$*" >&2; exit 1; }

# True when $1 doesn't exist or any of the other paths changed after it
outdated() {
    local target="$1"
    shift
    [ ! -e "$target" ] || [ -n "$(find "$@" -newer "$target" -print -quit)" ]
}

# Clones a tag of a repo to dir (unless present) and verifies its commit, only the given paths
# are checked out
clone() {
    local url="$1" version="$2" commit="$3" dir="$4"
    shift 4
    [ -d "$dir" ] && return
    msg "Cloning $url $version"
    rm -rf "$dir.part"
    mkdir -p "$SRC_DIR"
    # Annotated release tags make git warn that the tag "is not a commit", which it is
    git -c advice.detachedHead=false clone -q --depth 1 --branch "$version" --filter=blob:none \
        --sparse "$url" "$dir.part" 2>&1 | grep -v 'is not a commit' >&2 || true
    git -C "$dir.part" sparse-checkout set --no-cone "$@"
    [ "$(git -C "$dir.part" rev-parse HEAD)" = "$commit" ] || die "Unexpected commit for $url $version"
    mv "$dir.part" "$dir"
}

# Copies the std sources of the nightly into a repo, its rust-src matches its compiler
copy_rust() {
    [ -d "$RUST_DIR" ] && return
    msg "Copying the std sources of Rust $rust_toolchain"
    rustup -q toolchain install --profile minimal -c rust-src "$rust_toolchain" > /dev/null ||
        die "Failed to install Rust $rust_toolchain"
    rm -rf "$RUST_DIR.part"
    mkdir -p "$RUST_DIR.part"
    cp -R "$(rustc "+$rust_toolchain" --print sysroot)/lib/rustlib/src/rust/library" "$RUST_DIR.part/library"
    git -C "$RUST_DIR.part" init -q
    git -C "$RUST_DIR.part" add -A
    git -C "$RUST_DIR.part" -c user.name=winxp-toolchain -c user.email=winxp-toolchain@localhost \
        commit -q -m "Rust $rust_toolchain std sources"
    mv "$RUST_DIR.part" "$RUST_DIR"
}

fetch() {
    local pids=() pid failed=0
    clone "$mingw_url" "$mingw_version" "$mingw_commit" "$MINGW_DIR" /mingw-w64-headers/ /mingw-w64-crt/ &
    pids+=($!)
    clone "$llvm_url" "$llvm_version" "$llvm_commit" "$LLVM_DIR" /compiler-rt/lib/builtins/ &
    pids+=($!)
    copy_rust &
    pids+=($!)
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=1
    done
    [ "$failed" = 0 ] || die "Failed to fetch the Windows XP toolchain dependencies"
}

# Applies the patches of a repo as commits on top of its upstream commit, again when they changed
patch() {
    local name="$1" dir="$SRC_DIR/$1"
    outdated "$dir/.patched" "$TOOLCHAIN_DIR/patches/$name" || return 0
    # Applying resets ../../target/winxp-toolchain/src/<name>/, so stop when it has work that isn't exported yet
    if [ -s "$dir/.patched" ] && { [ -n "$(git -C "$dir" status --porcelain --untracked-files=no)" ] ||
        [ "$(git -C "$dir" rev-parse HEAD)" != "$(cat "$dir/.patched")" ]; }; then
        die "../../target/winxp-toolchain/src/$name/ has changes that aren't in patches/$name/, run ./build.sh export-patches or remove ../../target/winxp-toolchain/src/$name/"
    fi
    msg "Applying $name patches"
    # git am keeps the authors of the patches, but needs a committer identity
    if [ -z "$(git -C "$dir" config user.email || true)" ]; then
        git -C "$dir" config user.name winxp-toolchain
        git -C "$dir" config user.email winxp-toolchain@localhost
    fi
    git -C "$dir" config commit.gpgsign false
    git -C "$dir" rev-parse -q --verify upstream > /dev/null || git -C "$dir" tag upstream
    git -C "$dir" am --abort > /dev/null 2>&1 || true
    git -C "$dir" reset -q --hard upstream
    if ! git -C "$dir" am -q --keep-cr --whitespace=nowarn "$TOOLCHAIN_DIR/patches/$name"/*.patch; then
        git -C "$dir" am --abort
        rm -f "$dir/.patched"
        die "Failed to apply the $name patches"
    fi
    git -C "$dir" rev-parse HEAD > "$dir/.patched"
}

export_patches() {
    [ -d "$RUST_DIR/.git" ] || die "No patched rust-std, run ./build.sh first"
    rm -f "$TOOLCHAIN_DIR"/patches/rust-std/*.patch
    git -C "$RUST_DIR" format-patch -q --no-numbered --zero-commit --no-signature \
        -o "$TOOLCHAIN_DIR/patches/rust-std" upstream..HEAD
    git -C "$RUST_DIR" rev-parse HEAD > "$RUST_DIR/.patched"
    msg "Exported $(git -C "$RUST_DIR" rev-list --count upstream..HEAD) rust-std patches"
}

# The mingw-w64 headers for Windows XP and its C runtime on msvcrt.dll, with the import
# libraries of the system DLLs
build_mingw() {
    # The resource directory of the toolchain has clang's own headers, like its intrinsics
    mkdir -p "$BUILD_DIR/clang"
    ln -sfn "$("${LLVM}clang" -print-resource-dir)/include" "$BUILD_DIR/clang/include"
    outdated "$SYSROOT/lib/crt2.o" "$TOOLCHAIN_DIR/build.sh" || return 0
    msg "Building mingw-w64"
    local objects_dir="$OBJECTS_DIR/mingw-w64"
    rm -rf "$objects_dir" "$SYSROOT/include" "$SYSROOT/lib"
    mkdir -p "$objects_dir/headers" "$objects_dir/crt"
    (cd "$objects_dir/headers" && "$MINGW_DIR/mingw-w64-headers/configure" -q --host=i686-w64-mingw32 \
        --prefix="$SYSROOT" --with-default-win32-winnt=0x501 --with-default-msvcrt=msvcrt > /dev/null &&
        make -s install > /dev/null) || die "Failed to build the mingw-w64 headers"
    (cd "$objects_dir/crt" && "$MINGW_DIR/mingw-w64-crt/configure" -q --host=i686-w64-mingw32 \
        --prefix="$SYSROOT" --with-default-msvcrt=msvcrt --enable-lib32 --disable-lib64 \
        --disable-libarm32 --disable-libarm64 CC="${LLVM}clang ${XP_CFLAGS[*]}" AR="${LLVM}llvm-ar" \
        RANLIB="${LLVM}llvm-ranlib" DLLTOOL="${LLVM}llvm-dlltool" CFLAGS= > "$objects_dir/crt.log" 2>&1 &&
        make -s -j"$(getconf _NPROCESSORS_ONLN)" install >> "$objects_dir/crt.log" 2>&1) ||
        { tail -20 "$objects_dir/crt.log" >&2; die "Failed to build the mingw-w64 C runtime"; }
}

# Compiles sources at the same time and archives them in the sysroot,
# usage: build_archive <library> <flags>... -- <sources>...
build_archive() {
    local library="$SYSROOT/lib/$1" objects_dir="$OBJECTS_DIR/${1%.a}" flags=() source object
    local pids=() objects=() failed=0
    shift
    while [ "$1" != -- ]; do
        flags+=("$1")
        shift
    done
    shift
    msg "Building $(basename "$library")"
    rm -rf "$objects_dir"
    mkdir -p "$objects_dir"
    for source in "$@"; do
        object="$objects_dir/$(basename "$source").o"
        "${LLVM}clang" "${flags[@]}" -c "$source" -o "$object" &
        pids+=($!)
        objects+=("$object")
    done
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=1
    done
    [ "$failed" = 0 ] || die "Failed to compile $(basename "$library")"
    rm -f "$library"
    "${LLVM}llvm-ar" rcs "$library" "${objects[@]}"
}

build_libraries() {
    local builtins="$LLVM_DIR/compiler-rt/lib/builtins" source sources=()
    if outdated "$SYSROOT/lib/libclang_rt.builtins.a" "$SYSROOT/lib/crt2.o" "$TOOLCHAIN_DIR/build.sh"; then
        # Like compiler-rt, build the bfloat16 builtins only when clang supports the type for this target.
        local has_bfloat16=false
        if printf '__bf16 identity(__bf16 value) { return value; }\n' |
            "${LLVM}clang" "${XP_CFLAGS[@]}" -x c -c -o /dev/null - 2>/dev/null; then
            has_bfloat16=true
        fi
        # The generic builtins, the x86 assembly ones replace those with the same name, without
        # the ones for other systems and the unwinder, which panic=abort doesn't need
        for source in "$builtins"/*.c; do
            case "$(basename "$source" .c)" in
                atomic | crtbegin | crtend | emutls | gcc_personality_v0 | os_version_check) continue ;;
                extendbfsf2 | truncdfbf2 | truncsfbf2 | truncxfbf2 | trunctfbf2)
                    $has_bfloat16 || continue ;;
            esac
            [ -e "$builtins/i386/$(basename "$source" .c).S" ] || sources+=("$source")
        done
        build_archive libclang_rt.builtins.a "${XP_CFLAGS[@]}" -- "${sources[@]}" "$builtins"/i386/*.S \
            "$builtins"/i386/fp_mode.c
        # clang looks for its runtime library in its resource directory
        mkdir -p "$BUILD_DIR/clang/lib/i686-w64-windows-gnu"
        ln -sf ../../../sysroot/lib/libclang_rt.builtins.a "$BUILD_DIR/clang/lib/i686-w64-windows-gnu/libclang_rt.builtins.a"
    fi
}

clean() {
    msg "Removing ../../target/winxp-toolchain/"
    rm -rf "$BUILD_DIR"
}

locked=false
lock() {
    $locked && return
    local lock="$BUILD_DIR/.lock" pid waiting=false
    mkdir -p "$BUILD_DIR"
    until mkdir "$lock" 2> /dev/null; do
        # A new lock gets its process ID right after it's made, so give it a second
        pid=$(cat "$lock/pid" 2> /dev/null || { sleep 1; cat "$lock/pid" 2> /dev/null; } || true)
        if [ -z "$pid" ] || ! kill -0 "$pid" 2> /dev/null; then
            msg "Removing the lock of a build that was stopped"
            rm -rf "$lock"
            continue
        fi
        $waiting || msg "Waiting for another build"
        waiting=true
        sleep 1
    done
    echo $$ > "$lock/pid"
    trap 'rm -rf "$BUILD_DIR/.lock"' EXIT
    locked=true
}

build() {
    lock
    fetch
    patch rust-std
    build_mingw
    build_libraries
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) build ;;
        export-patches) lock; export_patches ;;
        # The cargo wrapper builds the toolchain itself, so release the lock before it runs
        test) (build); "$TOOLCHAIN_DIR/bin/cargo" run --release --manifest-path "$TOOLCHAIN_DIR/test/rust/Cargo.toml" \
            --target-dir "$OBJECTS_DIR/test" -- one 'two words' ;;
        clean) lock; clean ;;
        *) die "Unknown command: $command" ;;
    esac
done
