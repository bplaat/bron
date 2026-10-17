#!/usr/bin/env bash
# Builds the Nintendo DS toolchain in ../../target/nds-toolchain/: fetches calico, libnds, libdvm, FatFs, the default
# ARM7 program, ndstool and LLVM's compiler-rt, applies the patches in <name>/patches/ and builds
# the libraries, our tiny libc, the ARM7 program and ndstool.
# The port build scripts run this before they build.
#
# Usage: ./build.sh [command]...
#   (none)          fetch, patch and build
#   export-patches  write the commits in target/<name>/ back to <name>/patches/
#   test            build test/ and run it headless in DeSmuME
#   desmume         build DeSmuME's library and desmume/runner.c to target/bin/desmume-runner
#   clean           remove target/
set -euo pipefail
cd "$(dirname "$0")"

# name url version commit [sparse paths], every repo with a <name>/patches/ directory gets patched
REPOS=(
    'calico https://github.com/devkitPro/calico.git v1.2.0 741ca15cdb7e72de879c20d8f7877633caf92eaf'
    'libnds https://github.com/devkitPro/libnds.git v2.0.2 84e6082ce27c87ed218fb369a9944644aa2243a6'
    'libdvm https://github.com/devkitPro/libdvm.git v2.1.0 3006b93714b2cac7e052dbc44903b3ef29d2965a'
    'fatfs https://github.com/devkitPro/fatfs-mod.git v0.15.3 6d3e8f89202223bf70e957357dd31405b17d1f95'
    'default-arm7 https://github.com/devkitPro/default-arm7.git v0.8.4 90736cff9026e8f45b87c1b2edd59e197a09828a'
    'ndstool https://github.com/devkitPro/ndstool.git v2.3.1 76e8b681bb225d945a48852821e03114e6c7ce1c'
    'compiler-rt https://github.com/llvm/llvm-project.git llvmorg-23.1.3 0d261d1ca552c95a8f007e061c787ac7132fbcbc /compiler-rt/lib/builtins/'
)

# DeSmuME for desmume/runner.c, cloned by the desmume and test commands. Its last release is from
# 2022, so it's a commit of master, a version that is the commit fetches that commit.
DESMUME_REPO='desmume https://github.com/TASEmulators/desmume.git 1275dc64f5d1f5ef18dc1bc1fa9e012024b6df1a 1275dc64f5d1f5ef18dc1bc1fa9e012024b6df1a'

TOOLCHAIN_DIR="$PWD"
BUILD_DIR="$(cd ../.. && pwd)/target/nds-toolchain"
CALICO_DIR="$BUILD_DIR/calico"
LIBNDS_DIR="$BUILD_DIR/libnds"
LIBDVM_DIR="$BUILD_DIR/libdvm"
FATFS_DIR="$BUILD_DIR/fatfs"
BUILTINS_DIR="$BUILD_DIR/compiler-rt/compiler-rt/lib/builtins"

# Apple's clang can't target ARM bare metal, so prefer Homebrew's LLVM when it's installed
LLVM=''
for dir in /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/; do
    [ -d "$dir" ] && LLVM="$dir" && break
done

# The ARM9 is an ARM946E-S (ARMv5TE) and the ARM7 an ARM7TDMI (ARMv4T), both without floating
# point unit. There is no OS, so no standard library from the host. Code is Thumb unless the file
# is named *.32.c (calico's convention for ARM code), the linker script puts those in fast memory.
# ARMv4T can't switch between ARM and Thumb code in a direct call, so ARM7 calls are long calls
# through a register: lld's stubs for them can end up in the DSi only memory.
COMMON_CFLAGS=(-nostdlibinc -ffunction-sections -fdata-sections -g -D__NDS__
    -I"$TOOLCHAIN_DIR/libc/include" -I"$CALICO_DIR/include")
ARM9_CFLAGS=(--target=armv5te-none-eabi -mcpu=arm946e-s -mthumb -DARM9 -O2 "${COMMON_CFLAGS[@]}")
ARM7_CFLAGS=(--target=armv4t-none-eabi -mcpu=arm7tdmi -mthumb -mlong-calls -DARM7 -Os "${COMMON_CFLAGS[@]}")
LIB_CFLAGS=(-std=gnu17 -w -I"$LIBNDS_DIR/include" -I"$LIBDVM_DIR/include"
    -I"$FATFS_DIR/source")
# The libc is our own code, so it gets strict warnings, and no builtins: the compiler would turn
# the loops of memcpy and memset into calls to themselves
LIBC_CFLAGS=(-std=c23 -Wall -Wextra -ffreestanding)

CALICO_SOURCES=(
    system/{irq,tick,thread_cold,thread_hot.32,mutex,mailbox,dietprint,newlib_syscalls}.c
    arm/{arm-copy-fill.32,arm-context.32,arm-readtp.32}.s arm/arm-shims.32.c
    dev/fugu.32.c
    nds/{startup.crt0,tlnc.twl,pxi,smutex.32,keypad,pm,netbuf,gbacart,ntrcard,nitrorom}.c
    nds/{utils.crt0,bios,bios.twl,irq_handler.32}.s dev/wlan.c
)
CALICO9_SOURCES=(
    arm/arm-cache.32.s arm/arm-shims-mpu.32.c
    nds/arm9/{bootstub_arm9,mpu_setup.crt0,excpt_handler.32}.s dev/dldi_stub.s
    nds/arm9/{sys_startup,arm7_debug,pm_arm9,touch,sound,mic,blk,wlmgr,nitrorom,ovl}.c
)
CALICO7_SOURCES=(
    nds/arm7/bootstub_arm7.s
    nds/arm7/{debug,rtc,spi,pmic,nvram,tsc,env,pm_arm7,touch,mic.32,blk}.c
    nds/arm7/{scfg,i2c,mcu,codec,gpio}.twl.c nds/arm7/{tmio,sdmmc,sdio,blk}.twl.32.c
    nds/arm7/sound/{sound,sound_pxi}.c
)

# libnds without the on-screen keyboard, its graphics need grit
LIBNDS_SOURCES=(
    arm9/{background,boxtest,console,decompress,dynamicArray,exceptionDefault,gl2d,guitarGrip}.c
    arm9/{gurumeditation,heapfuncs,image,keys,linkedlist,paddle,pcx,piano,rampak,rumble,sassert}.c
    arm9/{shadowRegs,sound,sprite,sprite_alloc,system,trig,video,videoGL,videoGL_base,window}.c
    arm9/exceptionEntry.s arm9/system/cpu_clock.s
    common/{card,cardEeprom,dldi,rsa,sha1,timers}.c
)

# The compiler-rt builtins for what the ARM CPUs can't do in instructions: 64-bit integer math,
# division (the ports use the DS divider where it matters) and software floating point. The
# optimized ARM assembly floating point is used where it runs on ARMv5TE (see the patches).
BUILTINS_SOURCES=(
    {ashldi3,ashrdi3,lshrdi3,muldi3,negdi2,cmpdi2,ucmpdi2,mulodi4,mulosi4}.c
    {divdi3,moddi3,divmoddi4,udivdi3,umoddi3,udivmoddi4,ctzsi2,ctzdi2,clzdi2,popcountsi2}.c
    arm/{divsi3,udivsi3,modsi3,umodsi3,divmodsi4,udivmodsi4,clzsi2}.S
    arm/{aeabi_idivmod,aeabi_uidivmod,aeabi_ldivmod,aeabi_uldivmod}.S
    arm/{aeabi_memcmp,aeabi_memcpy,aeabi_memmove,aeabi_memset}.S arm/aeabi_div0.c
)
FLOAT_SOURCES=(
    arm/{addsf3,mulsf3,divsf3,adddf3,cmpsf2,cmpdf2,gesf2,gedf2,unordsf2,unorddf2}.S
    arm/{extendsfdf2,truncdfsf2,fixsfsi,fixdfsi,fixunssfsi,fixunsdfsi,fixsfdi,fixdfdi}.S
    arm/{fixunssfdi,fixunsdfdi,floatsisf,floatsidf,floatunsisf,floatunsidf,floatdisf}.S
    arm/{floatdidf,floatundidf}.S
    arm/{aeabi_fcmp,aeabi_dcmp,aeabi_cfcmp,aeabi_cdcmp}.S
    arm/{aeabi_cfcmpeq_check_nan,aeabi_cdcmpeq_check_nan,aeabi_frsub,aeabi_drsub}.c
    arm/{fnan2,fnorm2,funder,dnan2,dnorm2,dunder}.c
    muldf3.c divdf3.c floatundisf.c
)

msg() { printf '\033[1;34m[toolchain]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[toolchain]\033[0m %s\n' "$*" >&2; exit 1; }

# True when $1 doesn't exist or any of the other paths changed after it
outdated() {
    local target="$1"
    shift
    [ ! -e "$target" ] || [ -n "$(find "$@" -newer "$target" -print -quit)" ]
}

# Clones a tag of a repo to dir (unless present) and verifies its commit, when paths are given
# only those are checked out
clone() {
    local name="$1" url="$2" version="$3" commit="$4" dir="$BUILD_DIR/$1"
    shift 4
    [ -d "$dir" ] && return
    msg "Cloning $name $version"
    rm -rf "$dir.part"
    if [ "$version" = "$commit" ]; then
        git init -q "$dir.part"
        git -C "$dir.part" fetch -q --depth 1 "$url" "$commit"
        git -C "$dir.part" -c advice.detachedHead=false checkout -q FETCH_HEAD
    elif [ $# -gt 0 ]; then
        # LLVM's annotated release tags make git warn that the tag "is not a commit", which it is
        git -c advice.detachedHead=false clone -q --depth 1 --branch "$version" --filter=blob:none \
            --sparse "$url" "$dir.part" 2>&1 | grep -v 'is not a commit' >&2 || true
        git -C "$dir.part" sparse-checkout set --no-cone "$@"
    else
        git -c advice.detachedHead=false clone -q --depth 1 --branch "$version" "$url" "$dir.part" \
            2>&1 | grep -v 'is not a commit' >&2 || true
    fi
    [ "$(git -C "$dir.part" rev-parse HEAD)" = "$commit" ] || die "Unexpected commit for $name $version"
    mv "$dir.part" "$dir"
}

fetch() {
    local repo
    for repo in "${REPOS[@]}"; do
        # shellcheck disable=SC2086
        clone $repo &
    done
    wait
    for repo in "${REPOS[@]}"; do
        [ -d "$BUILD_DIR/${repo%% *}" ] || die "Failed to clone ${repo%% *}"
    done
}

# Applies the patches of a repo as commits on top of the upstream tag, again when they changed
patch() {
    local name="$1" commit="$2" dir="$BUILD_DIR/$1"
    outdated "$dir/.patched" "$TOOLCHAIN_DIR/$name/patches" || return 0
    # Applying resets target/<name>/, so stop when it has work that isn't exported yet
    if [ -s "$dir/.patched" ] && { [ -n "$(git -C "$dir" status --porcelain --untracked-files=no)" ] ||
        [ "$(git -C "$dir" rev-parse HEAD)" != "$(cat "$dir/.patched")" ]; }; then
        die "target/$name/ has changes that aren't in $name/patches/, run ./build.sh export-patches or remove target/$name/"
    fi
    msg "Applying $name patches"
    # git am keeps the authors of the patches, but needs a committer identity
    if [ -z "$(git -C "$dir" config user.email || true)" ]; then
        git -C "$dir" config user.name "Bastiaan van der Plaat"
        git -C "$dir" config user.email bastiaan.v.d.plaat@gmail.com
    fi
    git -C "$dir" config commit.gpgsign false
    git -C "$dir" tag -f upstream "$commit" > /dev/null
    # A patch that failed to apply leaves git am half done, start over
    git -C "$dir" am --abort > /dev/null 2>&1 || true
    git -C "$dir" reset -q --hard upstream
    # Preserve intentional CRLF in MIME-encoded patches without warning about it.
    if ! git -C "$dir" am -q --keep-cr --quoted-cr=nowarn --whitespace=nowarn "$TOOLCHAIN_DIR/$name"/patches/*.patch; then
        git -C "$dir" am --abort
        rm -f "$dir/.patched"
        die "Failed to apply the $name patches"
    fi
    # Remembers the applied commit, so the check above sees new work
    git -C "$dir" rev-parse HEAD > "$dir/.patched"
}

patch_all() {
    local repo name url version commit
    for repo in "${REPOS[@]}"; do
        read -r name url version commit _ <<< "$repo"
        [ -d "$TOOLCHAIN_DIR/$name/patches" ] && patch "$name" "$commit"
    done
    return 0
}

# Writes the commits on top of upstream back to <name>/patches/
export_patches() {
    local repo name dir
    for repo in "${REPOS[@]}" "$DESMUME_REPO"; do
        name="${repo%% *}"
        dir="$BUILD_DIR/$name"
        [ -d "$TOOLCHAIN_DIR/$name/patches" ] || continue
        # DeSmuME is only there once the desmume command ran
        [ "$repo" = "$DESMUME_REPO" ] && [ ! -d "$dir" ] && continue
        [ -d "$dir/.git" ] || die "No patched $name, run ./build.sh first"
        rm -f "$TOOLCHAIN_DIR/$name"/patches/*.patch
        git -C "$dir" format-patch -q --no-numbered --zero-commit --no-signature \
            -o "$TOOLCHAIN_DIR/$name/patches" upstream..HEAD
        git -C "$dir" rev-parse HEAD > "$dir/.patched"
        msg "Exported $(git -C "$dir" rev-list --count upstream..HEAD) $name patches"
    done
}

# Compiles sources at the same time and archives them to target/<library>, objects keep the
# suffixes of their source (like .32 and .twl) the linker scripts look for,
# usage: build_archive <library> <flags>... -- <sources>...
build_archive() {
    local library="$BUILD_DIR/$1" objects_dir="$BUILD_DIR/objects/${1%.a}" flags=() source object
    local pids=() objects=() failed=0 name
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
        name="$(basename "$source")"
        object="$objects_dir/$(basename "$(dirname "$source")")_${name%.*}.o"
        # clang can't inline calico's Thumb functions into its MK_CODE32 ARM functions, so files
        # with those are ARM code
        if [[ "$source" == *.[sS] ]]; then
            "${LLVM}clang" "${flags[@]}" -x assembler-with-cpp -c "$source" -o "$object" &
        elif [[ "$source" == *.32.c ]] || grep -q MK_CODE32 "$source"; then
            "${LLVM}clang" "${flags[@]}" -marm -c "$source" -o "$object" &
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
    local calico=("${CALICO_SOURCES[@]/#/$CALICO_DIR/source/}") libc=("$TOOLCHAIN_DIR"/libc/*.[cS])
    local builtins=("${BUILTINS_SOURCES[@]/#/$BUILTINS_DIR/}") floats=("${FLOAT_SOURCES[@]/#/$BUILTINS_DIR/}")
    # The library sources are checked too, so edits in target/ get built
    outdated "$BUILD_DIR/libcalico9.a" "$CALICO_DIR"/{source,include} "$TOOLCHAIN_DIR"/{libc/include,build.sh} &&
        build_archive libcalico9.a "${ARM9_CFLAGS[@]}" "${LIB_CFLAGS[@]}" -- \
            "${calico[@]}" "${CALICO9_SOURCES[@]/#/$CALICO_DIR/source/}"
    outdated "$BUILD_DIR/libcalico7.a" "$CALICO_DIR"/{source,include} "$TOOLCHAIN_DIR"/{libc/include,build.sh} &&
        build_archive libcalico7.a "${ARM7_CFLAGS[@]}" "${LIB_CFLAGS[@]}" -- \
            "${calico[@]}" "${CALICO7_SOURCES[@]/#/$CALICO_DIR/source/}"
    if outdated "$BUILD_DIR/libnds9.a" "$LIBNDS_DIR"/{source,include} "$TOOLCHAIN_DIR"/{libc/include,build.sh}; then
        # The console font is embedded as it is
        mkdir -p "$BUILD_DIR/generated"
        echo 'extern const unsigned char default_font_bin[];' > "$BUILD_DIR/generated/default_font_bin.h"
        printf '.section .rodata.default_font_bin, "a"\n.global default_font_bin\n.balign 4\ndefault_font_bin:\n.incbin "%s"\n' \
            "$LIBNDS_DIR/source/arm9/default_font.bin" > "$BUILD_DIR/generated/default_font.s"
        build_archive libnds9.a "${ARM9_CFLAGS[@]}" "${LIB_CFLAGS[@]}" -DNDEBUG -I"$BUILD_DIR/generated" -- \
            "${LIBNDS_SOURCES[@]/#/$LIBNDS_DIR/source/}" "$BUILD_DIR/generated/default_font.s"
    fi
    outdated "$BUILD_DIR/libdvm.a" "$LIBDVM_DIR"/{source,include} "$FATFS_DIR/source" \
        "$TOOLCHAIN_DIR"/{libc/include,build.sh} &&
        build_archive libdvm.a "${ARM9_CFLAGS[@]}" "${LIB_CFLAGS[@]}" -DLIBDVM_BUFFER_ALIGN=32 \
            -DLIBDVM_WITH_CACHE_COPY -DLIBDVM_WITH_ALIGNED_ACCESS -- \
            "$LIBDVM_DIR"/source/{dvm_disc,dvm_cache,dvm_volume,dvm_prober,fat_wrappers,fat_driver}.c \
            "$LIBDVM_DIR"/source/{dvm_calico,nitrofs}.c "$FATFS_DIR"/source/{ff,ffunicode}.c
    outdated "$BUILD_DIR/libc9.a" "$CALICO_DIR/include" "$TOOLCHAIN_DIR"/{libc,build.sh} &&
        build_archive libc9.a "${ARM9_CFLAGS[@]}" "${LIBC_CFLAGS[@]}" -- "${libc[@]}"
    outdated "$BUILD_DIR/libc7.a" "$CALICO_DIR/include" "$TOOLCHAIN_DIR"/{libc,build.sh} &&
        build_archive libc7.a "${ARM7_CFLAGS[@]}" "${LIBC_CFLAGS[@]}" -- "${libc[@]}"
    outdated "$BUILD_DIR/libcompiler-rt9.a" "$BUILTINS_DIR" "$TOOLCHAIN_DIR/build.sh" &&
        build_archive libcompiler-rt9.a "${ARM9_CFLAGS[@]}" -marm -std=gnu11 -w -- "${builtins[@]}" "${floats[@]}"
    outdated "$BUILD_DIR/libcompiler-rt7.a" "$BUILTINS_DIR" "$TOOLCHAIN_DIR/build.sh" &&
        build_archive libcompiler-rt7.a "${ARM7_CFLAGS[@]}" -marm -std=gnu11 -w -- "${builtins[@]}"
    return 0
}

# The default ARM7 program with the SD card (DSi) and DLDI storage and sound drivers, the ARM9 runs
# the ports and talks to it through calico
build_arm7() {
    local objects_dir="$BUILD_DIR/objects/ds7"
    outdated "$BUILD_DIR/ds7.elf" "$BUILD_DIR"/lib{calico7,c7,compiler-rt7}.a "$BUILD_DIR/default-arm7" \
        "$CALICO_DIR/share/ds7.ld" || return 0
    msg "Building ds7.elf"
    mkdir -p "$objects_dir"
    "${LLVM}clang" "${ARM7_CFLAGS[@]}" -Wall -DENABLE_BLKDEV -DENABLE_SOUND \
        -c "$BUILD_DIR/default-arm7/arm7_main.c" -o "$objects_dir/arm7_main.o"
    ld.lld -T "$CALICO_DIR/share/ds7.ld" --gc-sections --nmagic --section-start=.main=0x02ff0000 \
        "$objects_dir/arm7_main.o" "$BUILD_DIR"/lib{calico7,c7,compiler-rt7}.a -o "$BUILD_DIR/ds7.elf"
}

# ndstool runs on the host, it puts the ARM9 and ARM7 programs, the icon and NitroFS in a .nds
build_ndstool() {
    outdated "$BUILD_DIR/bin/ndstool" "$BUILD_DIR/ndstool/source" "$TOOLCHAIN_DIR/build.sh" || return 0
    msg "Building ndstool"
    local version source objects_dir="$BUILD_DIR/objects/ndstool"
    version="$(printf '%s\n' "${REPOS[@]}" | awk '$1 == "ndstool" { print substr($3, 2) }')"
    mkdir -p "$BUILD_DIR/bin" "$objects_dir"
    for source in "$BUILD_DIR"/ndstool/source/*.c; do
        cc -O2 -w -c "$source" -o "$objects_dir/$(basename "${source%.c}").o"
    done
    c++ -O2 -w -DPACKAGE_VERSION="\"$version\"" "$BUILD_DIR"/ndstool/source/*.cpp \
        "$objects_dir"/*.o -o "$BUILD_DIR/bin/ndstool"
}

# Builds test/ with the toolchain and runs it headless in desmume-runner, with an R4 card holding
# the ROM in test/target/sd/, the runner reads the results from RAM and fails when a check fails
run_test() {
    local target="$TOOLCHAIN_DIR/test/target" address
    build_desmume
    make -C "$TOOLCHAIN_DIR/test" --no-print-directory
    address=$("${LLVM}llvm-nm" "$target/test.elf" | awk '$3 == "test_results" { print $1 }')
    [ -n "$address" ] || die "Missing test results symbol"
    rm -rf "$target/sd"
    mkdir -p "$target/sd"
    cp "$target/test.nds" "$target/sd/test.nds"
    msg "Running the test in DeSmuME"
    "$BUILD_DIR/bin/desmume-runner" --fat "$target/sd" "$target/sd/test.nds" "test $address 3600"
    msg "All tests passed"
}

# Builds DeSmuME's interface library, the one py-desmume uses, and the runner on it, which plays a
# program headless and takes screenshots
build_desmume() {
    local interface="$BUILD_DIR/desmume/desmume/src/frontend/interface" objects_dir="$BUILD_DIR/objects/desmume"
    command -v meson > /dev/null || die "Install meson and ninja to build DeSmuME"
    # shellcheck disable=SC2086
    clone $DESMUME_REPO
    patch desmume "$(echo "$DESMUME_REPO" | cut -d' ' -f4)"
    [ -f "$objects_dir/build.ninja" ] || meson setup "$objects_dir" "$interface" --buildtype=release > /dev/null
    msg "Building DeSmuME"
    ninja -C "$objects_dir" > /dev/null
    outdated "$BUILD_DIR/bin/desmume-runner" "$TOOLCHAIN_DIR/desmume/runner.c" "$objects_dir"/libdesmume.* ||
        return 0
    msg "Building desmume-runner"
    mkdir -p "$BUILD_DIR/bin"
    cc -std=c11 -O2 -Wall -Wextra "$TOOLCHAIN_DIR/desmume/runner.c" -L"$objects_dir" -ldesmume -lz \
        -Wl,-rpath,"$objects_dir" -o "$BUILD_DIR/bin/desmume-runner"
}

# Only one build at a time, the ports can start one at the same time
locked=false
lock() {
    $locked && return
    mkdir -p "$BUILD_DIR"
    if ! mkdir "$BUILD_DIR/.lock" 2> /dev/null; then
        msg "Waiting for another build, remove target/.lock if none is running"
        until mkdir "$BUILD_DIR/.lock" 2> /dev/null; do sleep 1; done
    fi
    trap 'rmdir "$BUILD_DIR/.lock"' EXIT
    locked=true
}

[ $# -gt 0 ] || set -- all
for command in "$@"; do
    case "$command" in
        all) lock; fetch; patch_all; build_libraries; build_arm7; build_ndstool ;;
        export-patches) export_patches ;;
        test) lock; fetch; patch_all; build_libraries; build_arm7; build_ndstool; run_test ;;
        desmume) lock; build_desmume ;;
        clean) rm -rf "$BUILD_DIR" ;;
        *) die "Unknown command: $command" ;;
    esac
done
