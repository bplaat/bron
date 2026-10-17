#!/usr/bin/env bash
# Builds the Wii toolchain in the repository target/wii-toolchain/: fetches libogc, gamecube-tools and LLVM's compiler-rt,
# applies libogc/patches/ and builds libogc.a, libc.a (our tiny libc), libcompiler-rt.a,
# elf2dol and our lib/graphics/gxtexconv.
# The example Makefiles and Rust build.rs files run this before they build.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   export-patches  write the commits in ../../target/wii-toolchain/libogc/ back to libogc/patches/
#   test            build test/ and run it in Dolphin
#   clean           remove ../../target/wii-toolchain/
set -euo pipefail
cd "$(dirname "$0")"

libogc_url='https://github.com/devkitPro/libogc.git'
libogc_version='v2.11.0'
libogc_commit='7a2c05b8d78cf3a5ae2606874ef3e4a117273bf9'
tools_url='https://github.com/devkitPro/gamecube-tools.git'
tools_version='v1.0.7'
tools_commit='3b7f914df771d4b779d88607ceed2ec9264ff93a'
llvm_url='https://github.com/llvm/llvm-project.git'
llvm_version='llvmorg-23.1.3'
llvm_commit='0d261d1ca552c95a8f007e061c787ac7132fbcbc'

TOOLCHAIN_DIR="$PWD"
ROOT_DIR="$(cd ../.. && pwd)"
BUILD_DIR="$ROOT_DIR/target/wii-toolchain"
LIBOGC_DIR="$BUILD_DIR/libogc"
TOOLS_DIR="$BUILD_DIR/gamecube-tools"
COMPILER_RT_DIR="$BUILD_DIR/compiler-rt"

# Apple's clang can't target PowerPC, so prefer Homebrew's LLVM when it's installed
LLVM=''
for dir in /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/; do
    [ -d "$dir" ] && LLVM="$dir" && break
done

# The Wii has a PowerPC 750 CPU and there is no OS, so no standard library from the host.
# All code is optimized for size, like the Rust examples (opt-level = "s").
WII_CFLAGS=(--target=powerpc-none-eabi -mcpu=750 -ffreestanding -nostdlibinc
    -ffunction-sections -fdata-sections -DGEKKO -DHW_RVL -Os -g
    -I"$TOOLCHAIN_DIR/libc/include" -I"$LIBOGC_DIR/gc" -I"$LIBOGC_DIR/gc/ogc"
    -I"$LIBOGC_DIR/gc/ogc/machine" -I"$LIBOGC_DIR/gc/bte" -I"$LIBOGC_DIR/gc/wiiuse")
LIBOGC_CFLAGS=("${WII_CFLAGS[@]}" -std=gnu17 -fno-strict-aliasing -I"$LIBOGC_DIR/libogc"
    -Wall -Wno-parentheses -Wno-unused)
LIBC_CFLAGS=("${WII_CFLAGS[@]}" -std=c23 -Wall -Wextra)

# The parts of libogc we build, audio, DVD, memory cards, SD cards, networking and the console
# are left out because the examples don't use them and most need newlib
LIBOGC_SOURCES=(
    # Startup, interrupts, exceptions and caches
    libogc/{argv,cache,decrementer,exception,irq,system,sys_state,timesupp}.c
    libogc/{cache_asm,decrementer_handler,exception_handler,irq_handler,ogc_crt0,system_asm}.S
    # Threads
    libogc/{cond,lwp,lwp_heap,lwp_messages,lwp_mutex,lwp_objmgr,lwp_priority,lwp_queue}.c
    libogc/{lwp_sema,lwp_stack,lwp_threadq,lwp_threads,lwp_watchdog,lwp_wkspace,message,mutex,semaphore}.c
    libogc/lwp_handler.S
    # Video, GX graphics, matrix math and TPL textures
    libogc/{video,gx,gu,tpl}.c
    libogc/{video_asm,gu_psasm,gu_ps_concat44}.S
    # IOS, the Wii system software, and serial ports for the GameCube controllers
    libogc/{conf,es,exi,ios,ipc,isfs,pad,si,stm,usb,wiilaunch}.c
    # Bluetooth stack and the Wii Remote library
    lwbt/{bte,btmemb,btmemr,btpbuf,hci,l2cap,physbusif}.c
    wiiuse/{classic,dynamics,events,guitar_hero_3,io,io_wii,ir,motion_plus,nunchuk,speaker,wiiboard,wiiuse,wpad}.c
)

# The compiler-rt builtins for what the PowerPC 750 can't do in instructions: 64-bit integer
# math and conversions between 64-bit integers and floats. C needs these, Rust has its own.
COMPILER_RT_SOURCES=(
    {ashldi3,ashrdi3,lshrdi3,muldi3,negdi2,cmpdi2,ucmpdi2,mulodi4,mulosi4}.c
    {divdi3,moddi3,divmoddi4,udivdi3,umoddi3,udivmoddi4}.c
    {clzsi2,clzdi2,ctzsi2,ctzdi2,ffssi2,ffsdi2,popcountsi2,popcountdi2,paritysi2,paritydi2,bswapsi2,bswapdi2}.c
    {fixsfdi,fixunssfdi,fixdfdi,fixunsdfdi,fixunssfsi,fixunsdfsi}.c
    {floatdisf,floatundisf,floatdidf,floatundidf,floatunsisf,floatunsidf,powisf2,powidf2}.c
)

msg() { printf '\033[1;34m[toolchain]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[toolchain]\033[0m %s\n' "$*" >&2; exit 1; }
git_libogc() { git -C "$LIBOGC_DIR" "$@"; }

# True when $1 doesn't exist or any of the other paths changed after it
outdated() {
    local target="$1"
    shift
    [ ! -e "$target" ] || [ -n "$(find "$@" -newer "$target" -print -quit)" ]
}

# Clones a tag of a repo to dir (unless present) and verifies its commit, when paths are given
# only those are checked out
clone() {
    local url="$1" version="$2" commit="$3" dir="$4"
    shift 4
    [ -d "$dir" ] && return
    msg "Cloning $url $version"
    rm -rf "$dir.part"
    if [ $# -gt 0 ]; then
        # LLVM's annotated release tags make git warn that the tag "is not a commit", which it is
        git -c advice.detachedHead=false clone -q --depth 1 --branch "$version" --filter=blob:none \
            --sparse "$url" "$dir.part" 2>&1 | grep -v 'is not a commit' >&2 || true
        git -C "$dir.part" sparse-checkout set --no-cone "$@"
    else
        git -c advice.detachedHead=false clone -q --depth 1 --branch "$version" "$url" "$dir.part"
    fi
    [ "$(git -C "$dir.part" rev-parse HEAD)" = "$commit" ] || die "Unexpected commit for $url $version"
    mv "$dir.part" "$dir"
}

fetch() {
    local pids=() pid failed=0
    clone "$libogc_url" "$libogc_version" "$libogc_commit" "$LIBOGC_DIR" &
    pids+=($!)
    clone "$tools_url" "$tools_version" "$tools_commit" "$TOOLS_DIR" &
    pids+=($!)
    clone "$llvm_url" "$llvm_version" "$llvm_commit" "$COMPILER_RT_DIR" /compiler-rt/lib/builtins/ &
    pids+=($!)
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=1
    done
    [ "$failed" = 0 ] || die "Failed to clone Wii toolchain dependencies"
}

# Applies the patches as commits on top of the upstream tag, again when they changed
patch() {
    outdated "$LIBOGC_DIR/.patched" "$TOOLCHAIN_DIR"/libogc/patches || return 0
    # Applying resets ../../target/wii-toolchain/libogc/, so stop when it has work that isn't exported yet
    if [ -s "$LIBOGC_DIR/.patched" ] && { [ -n "$(git_libogc status --porcelain --untracked-files=no)" ] ||
        [ "$(git_libogc rev-parse HEAD)" != "$(cat "$LIBOGC_DIR/.patched")" ]; }; then
        die "../../target/wii-toolchain/libogc/ has changes that aren't in libogc/patches/, run ./build.sh export-patches or remove ../../target/wii-toolchain/libogc/"
    fi
    msg "Applying patches"
    # git am keeps the authors of the patches, but needs a committer identity
    if [ -z "$(git_libogc config user.email || true)" ]; then
        git_libogc config user.name wii-toolchain
        git_libogc config user.email wii-toolchain@localhost
    fi
    git_libogc config commit.gpgsign false
    git_libogc tag -f upstream "$libogc_commit" > /dev/null
    # A patch that failed to apply leaves git am half done, start over
    git_libogc am --abort > /dev/null 2>&1 || true
    git_libogc reset -q --hard upstream
    if ! git_libogc am -q --keep-cr --whitespace=nowarn "$TOOLCHAIN_DIR"/libogc/patches/*.patch; then
        git_libogc am --abort
        rm -f "$LIBOGC_DIR/.patched"
        die "Failed to apply the patches"
    fi
    # Remembers the applied commit, so the check above sees new work
    git_libogc rev-parse HEAD > "$LIBOGC_DIR/.patched"
}

# Writes the commits on top of upstream back to libogc/patches/
export_patches() {
    [ -d "$LIBOGC_DIR/.git" ] || die "No patched libogc, run ./build.sh first"
    rm -f "$TOOLCHAIN_DIR"/libogc/patches/*.patch
    git_libogc format-patch -q --no-numbered --zero-commit --no-signature \
        -o "$TOOLCHAIN_DIR/libogc/patches" upstream..HEAD
    git_libogc rev-parse HEAD > "$LIBOGC_DIR/.patched"
    msg "Exported $(git_libogc rev-list --count upstream..HEAD) patches"
}

# Compiles sources at the same time and archives them to the build directory,
# usage: build_archive <library> <flags>... -- <sources>...
build_archive() {
    local library="$BUILD_DIR/$1" objects_dir="$BUILD_DIR/objects/${1%.a}" flags=() source object
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
        object="$objects_dir/$(basename "$(dirname "$source")")_$(basename "$source").o"
        if [[ "$source" == *.S ]]; then
            "${LLVM}clang" "${flags[@]}" -D_LANGUAGE_ASSEMBLY -c "$source" -o "$object" &
        else
            "${LLVM}clang" "${flags[@]}" -c "$source" -o "$object" &
        fi
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
    # The libogc sources are checked too, so edits in dev mode get built
    if outdated "$BUILD_DIR/libogc.a" "$LIBOGC_DIR/.patched" "$LIBOGC_DIR"/{gc,libogc,lwbt,wiiuse} \
        "$TOOLCHAIN_DIR/libc/include" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libogc.a "${LIBOGC_CFLAGS[@]}" -- "${LIBOGC_SOURCES[@]/#/$LIBOGC_DIR/}"
    fi
    if outdated "$BUILD_DIR/libc.a" "$LIBOGC_DIR/.patched" "$LIBOGC_DIR/gc" "$TOOLCHAIN_DIR/libc" \
        "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libc.a "${LIBC_CFLAGS[@]}" -- "$TOOLCHAIN_DIR"/libc/*.c
    fi
    if outdated "$BUILD_DIR/libcompiler-rt.a" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libcompiler-rt.a "${WII_CFLAGS[@]}" -std=gnu11 -- \
            "${COMPILER_RT_SOURCES[@]/#/$COMPILER_RT_DIR/compiler-rt/lib/builtins/}"
    fi
}

# elf2dol from gamecube-tools and our lib/graphics/gxtexconv run on the host
build_tools() {
    if outdated "$BUILD_DIR/elf2dol" "$TOOLS_DIR/elftool"; then
        msg "Building elf2dol"
        cc -O2 "$TOOLS_DIR/elftool/elf2dol.c" -o "$BUILD_DIR/elf2dol"
    fi
    if outdated "$BUILD_DIR/gxtexconv" "$ROOT_DIR/lib/graphics/gxtexconv" "$ROOT_DIR/lib/graphics/image"; then
        msg "Building gxtexconv"
        # The Rust examples run this from their build.rs, so leave out their toolchain and target
        env -u RUSTUP_TOOLCHAIN -u RUSTC -u RUSTC_WRAPPER -u RUSTC_WORKSPACE_WRAPPER \
            -u CARGO_ENCODED_RUSTFLAGS -u CARGO_BUILD_TARGET \
            cargo build -q --release -p gxtexconv --manifest-path "$ROOT_DIR/Cargo.toml" \
            --target-dir "$ROOT_DIR/target" || die "Failed to build gxtexconv"
        cp "$ROOT_DIR/target/release/gxtexconv" "$BUILD_DIR/gxtexconv"
    fi
}

# Builds test/ with the toolchain and runs it in Dolphin without its user interface, the test
# writes its results to /tmp/test.txt on Dolphin's NAND. Doesn't lock, make runs the build itself.
run_test() {
    local test_dir="$TOOLCHAIN_DIR/test" test_build="$BUILD_DIR/test" user="$BUILD_DIR/test/dolphin" pid
    local results="$user/Wii/tmp/test.txt" nogui options=(--batch)
    # The Qt interface hangs without a GPU, so Linux uses the headless build with no video
    nogui=$(command -v dolphin-emu-nogui || echo /usr/games/dolphin-emu-nogui)
    if [ -z "${DOLPHIN:-}" ] && [ -x "$nogui" ]; then
        export DOLPHIN="$nogui"
        options=(--platform=headless --video_backend=Null)
    fi
    make -C "$test_dir" --no-print-directory
    msg "Running the test in Dolphin"
    rm -f "$results"
    # Without these a new user folder waits on dialogs
    mkdir -p "$user/Config"
    printf '[Analytics]\nPermissionAsked = True\n[Interface]\nUsePanicHandlers = False\n' \
        > "$user/Config/Dolphin.ini"
    "$TOOLCHAIN_DIR/run.sh" "$test_build/test.elf" "${options[@]}" --user="$user" \
        > "$test_build/dolphin.log" 2>&1 &
    pid=$!
    # Wait at most a minute for the test to finish
    for _ in $(seq 120); do
        grep -qs '^DONE' "$results" && break
        kill -0 "$pid" 2> /dev/null || break
        sleep 0.5
    done
    # Dolphin ignores the first SIGTERM
    kill -KILL "$pid" 2> /dev/null || true
    wait "$pid" 2> /dev/null || true
    cat "$results" 2> /dev/null || true
    grep -qs '^DONE 0 failed' "$results" ||
        { tail -n 20 "$test_build/dolphin.log" >&2; die "The test failed or didn't finish"; }
    msg "All tests passed"
}

# Only one build at a time, the C and Rust examples can start one at the same time
locked=false
lock() {
    $locked && return
    mkdir -p "$BUILD_DIR"
    if ! mkdir "$BUILD_DIR/.lock" 2> /dev/null; then
        msg "Waiting for another build, remove ../../target/wii-toolchain/.lock if none is running"
        until mkdir "$BUILD_DIR/.lock" 2> /dev/null; do sleep 1; done
    fi
    trap 'rmdir "$BUILD_DIR/.lock"' EXIT
    locked=true
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) lock; fetch; patch; build_libraries; build_tools ;;
        export-patches) export_patches ;;
        test) run_test ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
