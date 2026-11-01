#!/usr/bin/env bash
# Builds the Wii U toolchain in the repository target/wiiu-toolchain/: fetches wut, wut-tools,
# picolibc, libogc's matrix math, LLVM's compiler-rt and decaf-emu's latte-assembler, applies
# wut/patches/, wut-tools/patches/, picolibc/patches/, libogc/patches/ and decaf-emu/patches/ and
# builds libwut.a with the import stubs of the system libraries, libwhb and
# wutpicolibc, picolibc (the C library) in sysroot/, libogc's paired single matrix math in
# libgu.a, libcompiler-rt.a and the host tools elf2rpl, rplimportgen and latte-assembler.
# The example Makefiles and Rust build.rs files run this before they build.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   export-patches  write the commits in ../../target/wiiu-toolchain/<name>/ back to <name>/patches/
#   clean           remove ../../target/wiiu-toolchain/
set -euo pipefail
cd "$(dirname "$0")"

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

TOOLCHAIN_DIR="$PWD"
ROOT_DIR="$(cd ../.. && pwd)"
BUILD_DIR="$ROOT_DIR/target/wiiu-toolchain"
WUT_DIR="$BUILD_DIR/wut"
TOOLS_DIR="$BUILD_DIR/wut-tools"
PICOLIBC_DIR="$BUILD_DIR/picolibc"
LIBOGC_DIR="$BUILD_DIR/libogc"
COMPILER_RT_DIR="$BUILD_DIR/compiler-rt"
DECAF_DIR="$BUILD_DIR/decaf-emu"
SYSROOT="$BUILD_DIR/sysroot"

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
# picolibc its heap, console and clock, libwhb sets up the graphics and the process lifecycle
# and loads the shaders with libgfd
WUT_SOURCES=(
    libraries/wutpicolibc/wut_picolibc.c
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
        git -C "$dir.part" -c gc.auto=0 -c maintenance.auto=false submodule update -q --init --depth 1 $SUBMODULES
    fi
    mv "$dir.part" "$dir"
}

fetch() {
    local pids=() pid failed=0
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
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=1
    done
    [ "$failed" = 0 ] || die "Failed to clone Wii U toolchain dependencies"
}

# Applies the patches of a repo as commits on top of its upstream commit, again when they changed
patch() {
    local name="$1" commit="$2" dir="$BUILD_DIR/$1"
    outdated "$dir/.patched" "$TOOLCHAIN_DIR/$name/patches" || return 0
    # Applying resets ../../target/wiiu-toolchain/<name>/, so stop when it has work that isn't exported yet
    if [ -s "$dir/.patched" ] && { [ -n "$(git -C "$dir" status --porcelain --untracked-files=no)" ] ||
        [ "$(git -C "$dir" rev-parse HEAD)" != "$(cat "$dir/.patched")" ]; }; then
        die "../../target/wiiu-toolchain/$name/ has changes that aren't in $name/patches/, run ./build.sh export-patches or remove ../../target/wiiu-toolchain/$name/"
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
    if ! git -C "$dir" am -q --keep-cr --whitespace=nowarn "$TOOLCHAIN_DIR/$name"/patches/*.patch; then
        git -C "$dir" am --abort
        rm -f "$dir/.patched"
        die "Failed to apply the $name patches"
    fi
    # Remembers the applied commit, so the check above sees new work
    git -C "$dir" rev-parse HEAD > "$dir/.patched"
}

# Writes the commits on top of upstream back to <name>/patches/ for every patched repo
export_patches() {
    local name dir
    for name in wut wut-tools picolibc libogc decaf-emu; do
        dir="$BUILD_DIR/$name"
        [ -d "$dir/.git" ] || die "No patched $name, run ./build.sh first"
        rm -f "$TOOLCHAIN_DIR/$name"/patches/*.patch
        git -C "$dir" format-patch -q --no-numbered --zero-commit --no-signature \
            -o "$TOOLCHAIN_DIR/$name/patches" upstream..HEAD
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
    # Meson calls the 32-bit PowerPC CPU family ppc
    cat > "$BUILD_DIR/picolibc-cross.txt" << CROSS
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
    rm -rf "$BUILD_DIR/picolibc-build" "$SYSROOT"
    # Cafe OS starts the program, wutpicolibc gives picolibc its console and heap. The programs
    # are single threaded, so picolibc needs no locks or thread-local storage.
    meson setup "$BUILD_DIR/picolibc-build" "$PICOLIBC_DIR" \
        --cross-file "$BUILD_DIR/picolibc-cross.txt" --prefix "$SYSROOT" --includedir include \
        --libdir lib --buildtype plain -Dc_args="['-ffunction-sections', '-fdata-sections', '-DESPRESSO', '-Os', '-g']" \
        -Dmultilib=false -Dpicocrt=false -Dsemihost=false -Dtests=false -Dspecsdir=none \
        -Dthread-local-storage=false -Dsingle-thread=true -Dposix-console=false > /dev/null ||
        die "Failed to configure picolibc"
    meson install -C "$BUILD_DIR/picolibc-build" > /dev/null || die "Failed to build picolibc"
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

# rplimportgen writes the import stubs of every system library in wut's cafe/*.def files, the
# linker script includes imports.ld, which puts each library's stubs in its own import section
build_imports() {
    local def name
    outdated "$BUILD_DIR/imports.ld" "$BUILD_DIR/rplimportgen" "$WUT_DIR/cafe" "$TOOLCHAIN_DIR/build.sh" || return 0
    msg "Generating the import stubs"
    rm -rf "$BUILD_DIR/imports"
    mkdir -p "$BUILD_DIR/imports"
    for def in "$WUT_DIR"/cafe/*.def; do
        name="$(basename "$def" .def)"
        "$BUILD_DIR/rplimportgen" "$def" "$BUILD_DIR/imports/$name.S" "$BUILD_DIR/imports/$name.ld" > /dev/null ||
            die "Failed to generate the $name import stubs"
    done
    cat "$BUILD_DIR"/imports/*.ld > "$BUILD_DIR/imports.ld"
}

build_libraries() {
    # The wut sources are checked too, so edits in dev mode get built
    if outdated "$BUILD_DIR/libwut.a" "$WUT_DIR"/{.patched,include,libraries} "$BUILD_DIR/imports.ld" \
        "$SYSROOT/lib/libc.a" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libwut.a "${WIIU_CFLAGS[@]}" -std=gnu17 -Wall -Wno-unused -Wno-format-security -- \
            "$BUILD_DIR"/imports/*.S "${WUT_SOURCES[@]/#/$WUT_DIR/}"
    fi
    # The programs include gu.h from include/, with the two files it needs
    if outdated "$BUILD_DIR/libgu.a" "$LIBOGC_DIR"/{.patched,gc,libogc} "$SYSROOT/lib/libc.a" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libgu.a "${GU_CFLAGS[@]}" -- "${GU_SOURCES[@]/#/$LIBOGC_DIR/}"
        rm -rf "$BUILD_DIR/include"
        mkdir -p "$BUILD_DIR/include"
        cp "${GU_HEADERS[@]/#/$LIBOGC_DIR/}" "$BUILD_DIR/include/"
    fi
    if outdated "$BUILD_DIR/libcompiler-rt.a" "$TOOLCHAIN_DIR/build.sh"; then
        build_archive libcompiler-rt.a "${WIIU_CFLAGS[@]}" -std=gnu11 -- \
            "${COMPILER_RT_SOURCES[@]/#/$COMPILER_RT_DIR/compiler-rt/lib/builtins/}"
    fi
}

# elf2rpl and rplimportgen from wut-tools and latte-assembler from decaf-emu run on the host
build_tools() {
    local source objects_dir="$BUILD_DIR/objects/latte-assembler" pids=() objects=() pid failed=0
    if outdated "$BUILD_DIR/elf2rpl" "$TOOLS_DIR/.patched" "$TOOLS_DIR/src"; then
        msg "Building elf2rpl"
        c++ -std=c++17 -O2 -w -DFMT_HEADER_ONLY -I"$TOOLS_DIR/src/common" -I"$TOOLS_DIR/libraries/excmd/src" \
            -I"$TOOLS_DIR/libraries/fmt/include" "$TOOLS_DIR/src/elf2rpl/main.cpp" -lz -o "$BUILD_DIR/elf2rpl" ||
            die "Failed to build elf2rpl"
    fi
    if outdated "$BUILD_DIR/rplimportgen" "$TOOLS_DIR/.patched" "$TOOLS_DIR/src"; then
        msg "Building rplimportgen"
        c++ -std=c++17 -O2 -w -I"$TOOLS_DIR/src/common" "$TOOLS_DIR/src/rplimportgen/rplimportgen.cpp" \
            -o "$BUILD_DIR/rplimportgen" || die "Failed to build rplimportgen"
    fi
    outdated "$BUILD_DIR/latte-assembler" "$DECAF_DIR/.patched" "$DECAF_DIR/tools" "$TOOLCHAIN_DIR/build.sh" || return 0
    msg "Building latte-assembler"
    rm -rf "$objects_dir"
    mkdir -p "$objects_dir"
    # The assembler parses with this PEG grammar, upstream CMake generates the same header
    {
        printf 'const char *LatteGrammar = R"(\n'
        cat "$DECAF_DIR/tools/latte-assembler/resources/grammar.txt"
        printf ')";\n'
    } > "$objects_dir/grammar.h"
    for source in "${LATTE_ASSEMBLER_SOURCES[@]}"; do
        c++ -std=c++17 -O2 -w -DFMT_HEADER_ONLY -I"$objects_dir" -I"$DECAF_DIR/src" -I"$DECAF_DIR/src/libgpu" -I"$DECAF_DIR/src/libgfd" \
            -I"$DECAF_DIR/tools/latte-assembler/src" -I"$DECAF_DIR/libraries/fmt/include" \
            -I"$DECAF_DIR/libraries/excmd/src" -I"$DECAF_DIR/libraries/cpp-peglib" \
            -I"$DECAF_DIR/libraries/cnl/include" \
            -c "$DECAF_DIR/$source" -o "$objects_dir/$(basename "$source").o" &
        pids+=($!)
        objects+=("$objects_dir/$(basename "$source").o")
    done
    for pid in "${pids[@]}"; do
        wait "$pid" || failed=1
    done
    [ "$failed" = 0 ] || die "Failed to compile latte-assembler"
    c++ "${objects[@]}" -o "$BUILD_DIR/latte-assembler" || die "Failed to link latte-assembler"
}

# Only one build at a time, the C and Rust examples can start one at the same time
locked=false
lock() {
    $locked && return
    mkdir -p "$BUILD_DIR"
    if ! mkdir "$BUILD_DIR/.lock" 2> /dev/null; then
        msg "Waiting for another build, remove ../../target/wiiu-toolchain/.lock if none is running"
        until mkdir "$BUILD_DIR/.lock" 2> /dev/null; do sleep 1; done
    fi
    trap 'rmdir "$BUILD_DIR/.lock"' EXIT
    locked=true
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) lock; fetch; patch wut "$wut_commit"; patch wut-tools "$tools_commit"
            patch picolibc "$picolibc_commit"; patch libogc "$libogc_commit"; patch decaf-emu "$decaf_commit"
            build_picolibc; build_tools; build_imports; build_libraries ;;
        export-patches) export_patches ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
