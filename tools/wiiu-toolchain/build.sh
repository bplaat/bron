#!/usr/bin/env bash
# Builds the Wii U toolchain in the repository target/wiiu-toolchain/: fetches wut, wut-tools,
# picolibc, libogc's matrix math, LLVM's compiler-rt and decaf-emu's latte-assembler to src/,
# applies patches/<name>/ and builds libwut.a with the import stubs of the system libraries, libwhb
# and wutpicolibc, picolibc (the C library) in sysroot/, libogc's paired single matrix math in
# libgu.a and libcompiler-rt.a to lib/ and the host tools elf2rpl, rplimportgen, latte-assembler
# and our crates/gtxconv/ and crates/wuhbtool/ to bin/, with the intermediate files in build/.
# The C Makefiles and Rust Cargo wrapper run this before building a program.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   std             prepare the patched Rust std sources in src/rust-std/
#   export-patches  write the commits in ../../target/wiiu-toolchain/src/<name>/ back to patches/<name>/
#   test            build test/c and test/rust and run them in cemu-runner
#   cemu            build Cemu's interpreter and Cafe OS with patches/cemu/ and runner/ to
#                   ../../target/wiiu-toolchain/bin/cemu-runner
#   clean           remove ../../target/wiiu-toolchain/
set -euo pipefail
cd "$(dirname "$0")"

rust_toolchain='nightly-2026-11-14'
wut_url='https://github.com/devkitPro/wut.git'
wut_version='v1.9.1'
wut_commit='2016e429c16dab366d902455b43f21ccb520e23d'
tools_url='https://github.com/devkitPro/wut-tools.git'
tools_version='v1.3.5'
tools_commit='07bc2575c5dc018d45e1af8800d00f12b67cb385'
picolibc_url='https://github.com/picolibc/picolibc.git'
picolibc_version='1.8.12'
picolibc_commit='2ae376c6cdf4fef90ca2388ecf7a07457fa63cff'
libogc_url='https://github.com/devkitPro/libogc.git'
libogc_version='v2.11.0'
libogc_commit='7a2c05b8d78cf3a5ae2606874ef3e4a117273bf9'
llvm_url='https://github.com/llvm/llvm-project.git'
llvm_version='llvmorg-23.1.3'
llvm_commit='0d261d1ca552c95a8f007e061c787ac7132fbcbc'
# decaf-emu has no releases, its last commit has the latte-assembler we use
decaf_url='https://github.com/decaf-emu/decaf-emu.git'
decaf_version='master'
decaf_commit='e6c528a20a41c34e0f9eb91dd3da40f119db2dee'
# Cemu, fmt and pugixml for runner/runner.cpp, cloned by the cemu and test commands, only Cemu's
# interpreter and Cafe OS are built
cemu_url='https://github.com/cemu-project/Cemu.git'
cemu_version='main'
cemu_commit='ac8379ba66e8d149564f003569d9441464e7b228'
fmt_url='https://github.com/fmtlib/fmt.git'
fmt_version='12.0.0'
fmt_commit='e424e3f2e607da02742f73db84873b8084fc714c'
pugixml_url='https://github.com/zeux/pugixml.git'
pugixml_version='v1.15'
pugixml_commit='ee86beb30e4973f5feffe3ce63bfa4fbadf72f38'

TOOLCHAIN_DIR="$PWD"
ROOT_DIR="$(cd ../.. && pwd)"
BUILD_DIR="$ROOT_DIR/target/wiiu-toolchain"
SRC_DIR="$BUILD_DIR/src"
OBJ_DIR="$BUILD_DIR/build"
LIB_DIR="$BUILD_DIR/lib"
BIN_DIR="$BUILD_DIR/bin"
WUT_DIR="$SRC_DIR/wut"
TOOLS_DIR="$SRC_DIR/wut-tools"
PICOLIBC_DIR="$SRC_DIR/picolibc"
LIBOGC_DIR="$SRC_DIR/libogc"
COMPILER_RT_DIR="$SRC_DIR/compiler-rt"
CEMU_DIR="$SRC_DIR/cemu"
FMT_DIR="$SRC_DIR/fmt"
PUGIXML_DIR="$SRC_DIR/pugixml"
DECAF_DIR="$SRC_DIR/decaf-emu"
SYSROOT="$BUILD_DIR/sysroot"
RUST_DIR="$SRC_DIR/rust-std"

# Apple's clang can't target PowerPC, so prefer Homebrew's LLVM when it's installed
LLVM=''
for dir in /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/; do
    [ -d "$dir" ] && LLVM="$dir" && break
done

# The Wii U has a PowerPC 750 compatible Espresso CPU and Cafe OS, which only loads RPX files that
# elf2rpl makes from static ELF files. All code is optimized for size, like the Rust examples.
WIIU_CFLAGS=(--target=powerpc-none-eabi -mcpu=750 -ffreestanding -nostdlibinc
    -ffunction-sections -fdata-sections -DESPRESSO -D__WIIU__ -D__WUT__ -D_DEFAULT_SOURCE -Os -g
    -I"$SYSROOT/include" -I"$WUT_DIR/include" -I"$WUT_DIR/libraries/libwhb/include"
    -I"$WUT_DIR/libraries/libgfd/include")

# The parts of wut we build besides the import stubs: wutpicolibc starts the program and gives
# picolibc its heap, console, clock, sleep, locks and SD card files, libwhb sets up the graphics
# and the process lifecycle and loads the shaders with libgfd
WUT_SOURCES=(
    libraries/wutpicolibc/{wut_picolibc,wut_files,wut_entropy}.c
    libraries/libwhb/src/{gfx,gfx_heap,gfx_shader,gfx_texture,proc,log,log_cafe}.c
    libraries/libgfd/src/gfd.c
)

# libogc's matrix and vector math, its ps_ functions use the paired single instructions that the
# Espresso CPU has like the Wii's Broadway. Espresso is Gekko compatible, so GEKKO declares the ps_
# functions in gu.h.
GU_SOURCES=(libogc/gu.c libogc/{gu_psasm,gu_ps_concat44}.S)
GU_HEADERS=(gc/gctypes.h gc/ogc/gu.h gc/ogc/machine/paired_single.inc)
GU_CFLAGS=("${WIIU_CFLAGS[@]}" -DGEKKO -std=gnu17 -I"$LIBOGC_DIR/gc" -I"$LIBOGC_DIR/gc/ogc"
    -I"$LIBOGC_DIR/gc/ogc/machine" -Wall -Wno-unused)

# The compiler-rt builtins for what the PowerPC 750 can't do in instructions: 64-bit integer
# math and conversions between 64-bit integers and floats. C needs these, Rust has its own.
COMPILER_RT_SOURCES=(
    {ashldi3,ashrdi3,lshrdi3,muldi3,negdi2,cmpdi2,ucmpdi2,mulodi4,mulosi4}.c
    {divdi3,moddi3,divmoddi4,udivdi3,umoddi3,udivmoddi4}.c
    {clzsi2,clzdi2,ctzsi2,ctzdi2,ffssi2,ffsdi2,popcountsi2,popcountdi2,paritysi2,paritydi2,bswapsi2,bswapdi2}.c
    {fixsfdi,fixunssfdi,fixdfdi,fixunsdfdi,fixunssfsi,fixunsdfsi}.c
    {floatdisf,floatundisf,floatdidf,floatundidf,floatunsisf,floatunsidf,powisf2,powidf2}.c
)

# The decaf-emu sources latte-assembler needs
LATTE_ASSEMBLER_SOURCES=(
    tools/latte-assembler/src/{assembler_alu,assembler_cf,assembler_common,assembler_exp}.cpp
    tools/latte-assembler/src/{assembler_instructions,assembler_latte,assembler_parse,assembler_tex}.cpp
    tools/latte-assembler/src/{gfd,gfd_psh_comment_parser,gfd_vsh_comment_parser,main}.cpp
    src/libgfd/src/{gfd_read,gfd_write}.cpp
)

# The Cemu sources runner/runner.cpp runs programs with besides coreinit: the interpreter, the RPL
# loader, the FSA of IOSU on host folders, memory and the scheduler's fibers, without the GPU,
# audio, input, JIT or GUI
CEMU_SOURCES=(
    Cafe/HW/Espresso/Interpreter/PPCInterpreter{FPU,HLE,Impl,Main,OPC,PS}.cpp
    Cafe/HW/Espresso/{PPCScheduler,PPCTimer}.cpp Cafe/HW/Espresso/Debugger/GDBBreakpoints.cpp Cafe/HW/MMU/MMU.cpp
    Cafe/OS/common/OSCommon.cpp Cafe/OS/RPL/{elf,rpl,rpl_debug_symbols,rpl_symbol_storage}.cpp
    Cafe/OS/libs/nlibnss/nlibnss.cpp Cafe/OS/libs/nsysnet/nsysnet.cpp
    Cafe/IOSU/fsa/iosu_fsa.cpp Cafe/IOSU/kernel/iosu_kernel.cpp Cafe/Filesystem/{fsc,fscDeviceHostFS,fscDeviceRedirect}.cpp
    Common/{CafeStaticVar,SysAllocator,cpu_features}.cpp Common/unix/{FileStream_unix,platform}.cpp util/Fiber/FiberUnix.cpp
    util/MemMapper/MemMapperUnix.cpp util/highresolutiontimer/HighResolutionTimer.cpp util/crypto/crc32.cpp
)

msg() { printf '\033[1;34m[toolchain]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[toolchain]\033[0m %s\n' "$*" >&2; exit 1; }

# True when $1 doesn't exist or any of the other paths changed after it
outdated() {
    local target="$1"
    shift
    [ ! -e "$target" ] || [ -n "$(find "$@" -newer "$target" -print -quit)" ]
}

# Fetches one commit of a repo to dir (unless present), when paths are given only those are
# checked out. Submodules are fetched by setting SUBMODULES to their paths.
clone() {
    local url="$1" version="$2" commit="$3" dir="$4"
    shift 4
    [ -d "$dir" ] && return
    msg "Cloning $url $version"
    rm -rf "$dir.part"
    git init -q "$dir.part"
    git -C "$dir.part" remote add origin "$url"
    if [ $# -gt 0 ]; then
        git -C "$dir.part" fetch -q --depth 1 --filter=blob:none origin "$commit"
        git -C "$dir.part" sparse-checkout set --no-cone "$@"
    else
        git -C "$dir.part" fetch -q --depth 1 origin "$commit"
    fi
    git -C "$dir.part" -c advice.detachedHead=false checkout -q FETCH_HEAD
    if [ -n "${SUBMODULES:-}" ]; then
        # Without auto maintenance, which would keep writing to $dir.part in the background
        # shellcheck disable=SC2086
        git -C "$dir.part" -c gc.auto=0 -c maintenance.auto=false submodule update -q --init --depth 1 --jobs 8 $SUBMODULES
    fi
    mv "$dir.part" "$dir"
}

# Fetches only the dependencies needed by this command, before any builds start.
fetch() {
    local mode="${1:-all}" pids=() pid failed=0
    mkdir -p "$SRC_DIR"
    if [ "$mode" != cemu ]; then
        clone "$wut_url" "$wut_version" "$wut_commit" "$WUT_DIR" &
        pids+=($!)
        SUBMODULES='libraries/fmt libraries/excmd' clone "$tools_url" "$tools_version" "$tools_commit" "$TOOLS_DIR" &
        pids+=($!)
        clone "$picolibc_url" "$picolibc_version" "$picolibc_commit" "$PICOLIBC_DIR" &
        pids+=($!)
        clone "$libogc_url" "$libogc_version" "$libogc_commit" "$LIBOGC_DIR" /gc/gctypes.h /gc/ogc/gu.h \
            /gc/ogc/machine/ /libogc/gu.c /libogc/gu_psasm.S /libogc/gu_ps_concat44.S &
        pids+=($!)
        clone "$llvm_url" "$llvm_version" "$llvm_commit" "$COMPILER_RT_DIR" /compiler-rt/lib/builtins/ &
        pids+=($!)
        SUBMODULES='libraries/cnl libraries/fmt libraries/excmd libraries/cpp-peglib' clone "$decaf_url" "$decaf_version" \
            "$decaf_commit" "$DECAF_DIR" /tools/latte-assembler/ /src/libgfd/ /src/libgpu/ /src/libcpu/ /src/common/ \
            /src/libdecaf/src/cafe/libraries/gx2/ /libraries/ &
        pids+=($!)
    fi
    if [ "$mode" != all ]; then
        clone "$cemu_url" "$cemu_version" "$cemu_commit" "$CEMU_DIR" /src/ &
        pids+=($!)
        clone "$fmt_url" "$fmt_version" "$fmt_commit" "$FMT_DIR" /include/ &
        pids+=($!)
        clone "$pugixml_url" "$pugixml_version" "$pugixml_commit" "$PUGIXML_DIR" /src/ &
        pids+=($!)
    fi
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=1
    done
    [ "$failed" = 0 ] || die "Failed to clone Wii U toolchain dependencies"
}

# The standard library sources must match the compiler that builds them.
copy_rust() {
    # The cargo wrapper runs this nightly, also when the Rust sources come from a cache
    rustup toolchain list | grep -q "^$rust_toolchain-" ||
        rustup -q toolchain install --profile minimal -c rust-src "$rust_toolchain"
    if [ -d "$RUST_DIR" ]; then
        [ "$(cat "$RUST_DIR/.toolchain")" = "$rust_toolchain" ] ||
            die "Rust nightly changed, export any local Rust patches and remove $RUST_DIR"
        return
    fi
    msg "Copying the std sources of Rust $rust_toolchain"
    rm -rf "$RUST_DIR.part"
    mkdir -p "$RUST_DIR.part"
    cp -R "$(rustc "+$rust_toolchain" --print sysroot)/lib/rustlib/src/rust/library" "$RUST_DIR.part/library"
    git -C "$RUST_DIR.part" init -q
    git -C "$RUST_DIR.part" add library
    git -C "$RUST_DIR.part" -c commit.gpgsign=false -c user.name=wiiu-toolchain -c user.email=wiiu-toolchain@localhost \
        commit -q -m "Rust $rust_toolchain std sources"
    git -C "$RUST_DIR.part" tag upstream
    printf '%s\n' "$rust_toolchain" > "$RUST_DIR.part/.toolchain"
    mv "$RUST_DIR.part" "$RUST_DIR"
}

# Applies the patches of a repo as commits on top of its upstream commit, again when they changed
patch() {
    local name="$1" commit="$2" dir="$SRC_DIR/$1"
    outdated "$dir/.patched" "$TOOLCHAIN_DIR/patches/$name" || return 0
    # Applying resets ../../target/wiiu-toolchain/src/<name>/, so stop when it has work that isn't exported yet
    if [ -s "$dir/.patched" ] && { [ -n "$(git -C "$dir" status --porcelain --untracked-files=no)" ] ||
        [ "$(git -C "$dir" rev-parse HEAD)" != "$(cat "$dir/.patched")" ]; }; then
        die "../../target/wiiu-toolchain/src/$name/ has changes that aren't in patches/$name/, run ./build.sh export-patches or remove ../../target/wiiu-toolchain/src/$name/"
    fi
    msg "Applying $name patches"
    # git am keeps the authors of the patches, but needs a committer identity
    if [ -z "$(git -C "$dir" config user.email || true)" ]; then
        git -C "$dir" config user.name wiiu-toolchain
        git -C "$dir" config user.email wiiu-toolchain@localhost
    fi
    git -C "$dir" config commit.gpgsign false
    git -C "$dir" tag -f upstream "$commit" > /dev/null
    # A patch that failed to apply leaves git am half done, start over
    git -C "$dir" am --abort > /dev/null 2>&1 || true
    git -C "$dir" reset -q --hard upstream
    if ! git -C "$dir" am -q --keep-cr --whitespace=nowarn "$TOOLCHAIN_DIR/patches/$name"/*.patch; then
        git -C "$dir" am --abort
        rm -f "$dir/.patched"
        die "Failed to apply the $name patches"
    fi
    # Remembers the applied commit, so the check above sees new work
    git -C "$dir" rev-parse HEAD > "$dir/.patched"
}

# Writes the commits on top of upstream back to patches/<name>/ for every patched repo
export_patches() {
    local name dir
    for name in wut wut-tools picolibc libogc decaf-emu cemu rust-std; do
        dir="$SRC_DIR/$name"
        # Cemu and Rust are only there once the cemu or std command ran
        [[ "$name" == cemu || "$name" == rust-std ]] && [ ! -d "$dir" ] && continue
        [ -d "$dir/.git" ] || die "No patched $name, run ./build.sh first"
        rm -f "$TOOLCHAIN_DIR/patches/$name"/*.patch
        git -C "$dir" format-patch -q --no-numbered --zero-commit --no-signature \
            -o "$TOOLCHAIN_DIR/patches/$name" upstream..HEAD
        git -C "$dir" rev-parse HEAD > "$dir/.patched"
        msg "Exported $(git -C "$dir" rev-list --count upstream..HEAD) $name patches"
    done
}

# Builds picolibc with meson and installs its headers and libc.a, which includes the math
# library, in ../../target/wiiu-toolchain/sysroot/
build_picolibc() {
    # The sources are checked too, so edits in dev mode get built
    outdated "$SYSROOT/lib/libc.a" "$PICOLIBC_DIR/.patched" "$PICOLIBC_DIR"/{libc,libm,meson.build} \
        "$TOOLCHAIN_DIR/build.sh" || return 0
    msg "Building picolibc"
    mkdir -p "$OBJ_DIR"
    # Meson calls the 32-bit PowerPC CPU family ppc
    cat > "$OBJ_DIR/picolibc-cross.txt" << CROSS
[binaries]
c = ['${LLVM}clang', '--target=powerpc-none-eabi', '-mcpu=750', '-nostdlib']
c_ld = 'lld'
ar = '${LLVM}llvm-ar'
nm = '${LLVM}llvm-nm'
strip = '${LLVM}llvm-strip'

[host_machine]
system = 'none'
cpu_family = 'ppc'
cpu = '750'
endian = 'big'

[properties]
skip_sanity_check = true
CROSS
    rm -rf "$OBJ_DIR/picolibc" "$SYSROOT"
    # Cafe OS starts the program, wutpicolibc gives picolibc its console, heap, locks and files.
    # wutpicolibc supplies per-thread errno, and the streams lock and are flushed at exit.
    meson setup "$OBJ_DIR/picolibc" "$PICOLIBC_DIR" \
        --cross-file "$OBJ_DIR/picolibc-cross.txt" --prefix "$SYSROOT" --includedir include \
        --libdir lib --buildtype plain -Dc_args="['-ffunction-sections', '-fdata-sections', '-DESPRESSO', '-Os', '-g']" \
        -Dmultilib=false -Dpicocrt=false -Dsemihost=false -Dtests=false -Dspecsdir=none \
        -Dthread-local-storage=false -Derrno-function=__errno -Dposix-console=false -Dstdio-locking=true -Dstdio-exit-flush=true > /dev/null ||
        die "Failed to configure picolibc"
    meson install -C "$OBJ_DIR/picolibc" > /dev/null || die "Failed to build picolibc"
}

# Compiles sources at the same time and archives them to lib/,
# usage: build_archive <library> <flags>... -- <sources>...
build_archive() {
    local library="$LIB_DIR/$1" objects_dir="$OBJ_DIR/${1%.a}" flags=() source object
    local pids=() objects=() failed=0
    shift
    while [ "$1" != -- ]; do
        flags+=("$1")
        shift
    done
    shift
    msg "Building $(basename "$library")"
    rm -rf "$objects_dir"
    mkdir -p "$objects_dir" "$LIB_DIR"
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
    "${LLVM}llvm-ar" rcs "$objects_dir/archive.a" "${objects[@]}"
    mv -f "$objects_dir/archive.a" "$library"
}

# Compiles host C++ sources, as many at the same time as there are CPUs, and links them to a program
# in bin/, usage: build_host <program> <flags>... -- <sources>... -- <link flags>...
build_host() {
    local program="$1" objects_dir="$OBJ_DIR/$1" jobs flags=() objects=() pids=() source object pid failed=0
    jobs=$(getconf _NPROCESSORS_ONLN)
    shift
    while [ "$1" != -- ]; do
        flags+=("$1")
        shift
    done
    shift
    msg "Building $program"
    mkdir -p "$objects_dir" "$BIN_DIR"
    while [ "$1" != -- ]; do
        source="$1"
        shift
        object="$objects_dir/$(basename "$(dirname "$source")")_$(basename "$source").o"
        c++ "${flags[@]}" -c "$source" -o "$object" &
        pids+=($!)
        objects+=("$object")
        # Waits for a batch to finish before starting the next
        if [ "${#pids[@]}" -ge "$jobs" ] || [ "$1" = -- ]; then
            for pid in "${pids[@]}"; do
                wait "$pid" || failed=1
            done
            pids=()
        fi
    done
    shift
    [ "$failed" = 0 ] || die "Failed to compile $program"
    c++ "${objects[@]}" "$@" -o "$BIN_DIR/$program" || die "Failed to link $program"
}

# rplimportgen writes the import stubs of every system library in wut's cafe/*.def files, the
# linker script includes lib/imports.ld, which puts each library's stubs in its own import section
build_imports() {
    local def name
    outdated "$LIB_DIR/imports.ld" "$BIN_DIR/rplimportgen" "$WUT_DIR/cafe" "$TOOLCHAIN_DIR/build.sh" || return 0
    msg "Generating the import stubs"
    rm -rf "$OBJ_DIR/imports"
    mkdir -p "$OBJ_DIR/imports" "$LIB_DIR"
    for def in "$WUT_DIR"/cafe/*.def; do
        name="$(basename "$def" .def)"
        "$BIN_DIR/rplimportgen" "$def" "$OBJ_DIR/imports/$name.S" "$OBJ_DIR/imports/$name.ld" > /dev/null ||
            die "Failed to generate the $name import stubs"
    done
    cat "$OBJ_DIR"/imports/*.ld > "$LIB_DIR/imports.ld"
}

build_libraries() {
    # The wut sources are checked too, so edits in dev mode get built
    if outdated "$LIB_DIR/libwut.a" "$WUT_DIR"/{.patched,include,libraries} "$LIB_DIR/imports.ld" \
        "$SYSROOT/lib/libc.a" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libwut.a "${WIIU_CFLAGS[@]}" -std=gnu17 -Wall -Wno-unused -Wno-format-security -- \
            "$OBJ_DIR"/imports/*.S "${WUT_SOURCES[@]/#/$WUT_DIR/}"
    fi
    # The programs include gu.h from include/, with the two files it needs
    if outdated "$LIB_DIR/libgu.a" "$LIBOGC_DIR"/{.patched,gc,libogc} "$SYSROOT/lib/libc.a" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libgu.a "${GU_CFLAGS[@]}" -- "${GU_SOURCES[@]/#/$LIBOGC_DIR/}"
        rm -rf "$BUILD_DIR/include"
        mkdir -p "$BUILD_DIR/include"
        cp "${GU_HEADERS[@]/#/$LIBOGC_DIR/}" "$BUILD_DIR/include/"
    fi
    if outdated "$LIB_DIR/libcompiler-rt.a" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libcompiler-rt.a "${WIIU_CFLAGS[@]}" -std=gnu11 -- \
            "${COMPILER_RT_SOURCES[@]/#/$COMPILER_RT_DIR/compiler-rt/lib/builtins/}"
    fi
}

# elf2rpl and rplimportgen from wut-tools, latte-assembler from decaf-emu and our crates/gtxconv/
# and crates/wuhbtool/ run on the host
build_tools() {
    local objects_dir="$OBJ_DIR/latte-assembler" tool
    mkdir -p "$BIN_DIR"
    for tool in gtxconv wuhbtool; do
        outdated "$BIN_DIR/$tool" "$TOOLCHAIN_DIR/crates/$tool" "$ROOT_DIR/lib/graphics/image" "$ROOT_DIR/lib/data" ||
            continue
        msg "Building $tool"
        # Build scripts under the cargo wrapper reach this through the cc script, so leave out the
        # wrapper's toolchain, target and profiles
        env -u RUSTUP_TOOLCHAIN -u RUSTC -u RUSTC_WRAPPER -u RUSTC_WORKSPACE_WRAPPER \
            -u CARGO_ENCODED_RUSTFLAGS -u CARGO_BUILD_TARGET -u CARGO_UNSTABLE_BUILD_STD \
            -u CARGO_UNSTABLE_JSON_TARGET_SPEC -u __CARGO_TESTS_ONLY_SRC_ROOT \
            -u CARGO_PROFILE_RELEASE_OPT_LEVEL -u CARGO_PROFILE_RELEASE_PANIC \
            -u CARGO_PROFILE_RELEASE_LTO -u CARGO_PROFILE_RELEASE_CODEGEN_UNITS \
            -u CARGO_PROFILE_RELEASE_STRIP -u CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_OPT_LEVEL \
            cargo build -q --release -p "$tool" --manifest-path "$ROOT_DIR/Cargo.toml" \
            --target-dir "$ROOT_DIR/target" || die "Failed to build $tool"
        rm -f "$BIN_DIR/$tool"
        cp "$ROOT_DIR/target/release/$tool" "$BIN_DIR/$tool"
    done
    if outdated "$BIN_DIR/elf2rpl" "$TOOLS_DIR/.patched" "$TOOLS_DIR/src"; then
        msg "Building elf2rpl"
        c++ -std=c++17 -O2 -w -DFMT_HEADER_ONLY -I"$TOOLS_DIR/src/common" -I"$TOOLS_DIR/libraries/excmd/src" \
            -I"$TOOLS_DIR/libraries/fmt/include" "$TOOLS_DIR/src/elf2rpl/main.cpp" -lz -o "$BIN_DIR/elf2rpl" ||
            die "Failed to build elf2rpl"
    fi
    if outdated "$BIN_DIR/rplimportgen" "$TOOLS_DIR/.patched" "$TOOLS_DIR/src"; then
        msg "Building rplimportgen"
        c++ -std=c++17 -O2 -w -I"$TOOLS_DIR/src/common" "$TOOLS_DIR/src/rplimportgen/rplimportgen.cpp" \
            -o "$BIN_DIR/rplimportgen" || die "Failed to build rplimportgen"
    fi
    outdated "$BIN_DIR/latte-assembler" "$DECAF_DIR/.patched" "$DECAF_DIR/tools" "$TOOLCHAIN_DIR/build.sh" || return 0
    # The assembler parses with this PEG grammar, upstream CMake generates the same header
    rm -rf "$objects_dir"
    mkdir -p "$objects_dir"
    {
        printf 'const char *LatteGrammar = R"(\n'
        cat "$DECAF_DIR/tools/latte-assembler/resources/grammar.txt"
        printf ')";\n'
    } > "$objects_dir/grammar.h"
    build_host latte-assembler -std=c++17 -O2 -w -DFMT_HEADER_ONLY -I"$objects_dir" -I"$DECAF_DIR/src" \
        -I"$DECAF_DIR/src/libgpu" -I"$DECAF_DIR/src/libgfd" -I"$DECAF_DIR/tools/latte-assembler/src" \
        -I"$DECAF_DIR/libraries/fmt/include" -I"$DECAF_DIR/libraries/excmd/src" -I"$DECAF_DIR/libraries/cpp-peglib" \
        -I"$DECAF_DIR/libraries/cnl/include" -- "${LATTE_ASSEMBLER_SOURCES[@]/#/$DECAF_DIR/}" --
}

# Builds runner/runner.cpp with Cemu's interpreter and Cafe OS. Cemu's headers include boost, glm
# and wxWidgets for things the OS emulation doesn't use, runner/shim/ has the few parts it does.
# ENABLE_OPENGL only gives the unused graphics API setting a default.
# macOS only declares the ucontext functions of Cemu's fibers for X/Open sources.
build_cemu() {
    local flags=()
    patch cemu "$cemu_commit"
    outdated "$BIN_DIR/cemu-runner" "$TOOLCHAIN_DIR/runner" "$CEMU_DIR"/{.patched,src} "$TOOLCHAIN_DIR/build.sh" || return 0
    [ "$(uname)" = Darwin ] && flags+=(-D_XOPEN_SOURCE)
    build_host cemu-runner -std=c++20 -O2 -w -fno-strict-aliasing -DFMT_HEADER_ONLY -DEMULATOR_VERSION_MAJOR=2 \
        -DEMULATOR_VERSION_MINOR=6 -DEMULATOR_VERSION_PATCH=0 -DEMULATOR_HASH="$cemu_version" ${flags[@]+"${flags[@]}"} \
        -DENABLE_OPENGL -I"$CEMU_DIR/src" -I"$CEMU_DIR/src/Cafe" -I"$TOOLCHAIN_DIR/runner/shim" -I"$FMT_DIR/include" \
        -I"$PUGIXML_DIR/src" -include "$CEMU_DIR/src/Common/precompiled.h" -- \
        "${CEMU_SOURCES[@]/#/$CEMU_DIR/src/}" "$CEMU_DIR"/src/Cafe/OS/libs/coreinit/*.cpp \
        "$PUGIXML_DIR/src/pugixml.cpp" "$TOOLCHAIN_DIR/runner/runner.cpp" -- -lz -pthread
}

# Each guest reports through OSReport and gets its own user folder with an SD card.
# Release the lock before make or the Cargo wrapper runs the toolchain build.
run_test() (
    local user elf="$TOOLCHAIN_DIR/test/rust/target/powerpc-nintendo-wiiu/release/wiiu-std-test.elf" rpx status
    (lock; fetch test; build_cemu)
    make -C "$TOOLCHAIN_DIR/test/c" --no-print-directory
    user=$(mktemp -d)
    trap 'rm -rf "$user"' EXIT
    msg "Running the C suite in cemu-runner"
    "$BIN_DIR/cemu-runner" "$TOOLCHAIN_DIR/test/c/target/test.rpx" test 60 "$user/c"

    "$TOOLCHAIN_DIR/bin/cargo" build --locked --release --manifest-path "$TOOLCHAIN_DIR/test/rust/Cargo.toml"
    mkdir -p "$user/rust/sdcard"
    rpx="$user/rust/sdcard/boot.rpx"
    "$BIN_DIR/elf2rpl" "$elf" "$rpx" > /dev/null
    msg "Running the Rust suite in cemu-runner"
    "$BIN_DIR/cemu-runner" "$rpx" test 60 "$user/rust" 'suite words'

    msg "Checking panic output and abort in cemu-runner"
    status=0
    "$BIN_DIR/cemu-runner" "$rpx" run 10 "$user/panic" 'panic' > "$user/panic.log" 2>&1 || status=$?
    cat "$user/panic.log"
    [ "$status" = 134 ] && grep -q 'wiiu-std-panic-marker' "$user/panic.log" ||
        die "Rust panic did not print its message and exit with status 134"
    msg "All tests passed"
)

# Only one build at a time, the C and Rust examples can start one at the same time. The lock holds
# the process ID of its build, so the lock of a killed build is taken over instead of waited on.
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

unlock() {
    $locked || return 0
    rm -rf "$BUILD_DIR/.lock"
    trap - EXIT
    locked=false
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) lock; fetch; patch wut "$wut_commit"; patch wut-tools "$tools_commit"
            patch picolibc "$picolibc_commit"; patch libogc "$libogc_commit"; patch decaf-emu "$decaf_commit"
            build_picolibc; build_tools; build_imports; build_libraries ;;
        std) lock; copy_rust; patch rust-std upstream ;;
        test) unlock; run_test ;;
        cemu) lock; fetch cemu; build_cemu ;;
        export-patches) export_patches ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
