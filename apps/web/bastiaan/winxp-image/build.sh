#!/bin/sh
# Builds the Windows XP disk and desktop snapshot into web/public/winxp, see winxp-image/README.md
# Usage: winxp-image/build.sh           build what is missing
#        winxp-image/build.sh software  reinstall the software on the kept base install, e.g. after adding some
#        winxp-image/build.sh setup     redo the v86 setup on the kept software install
#        winxp-image/build.sh install   start over with a fresh Windows install
set -eu
cd "$(dirname "$0")"
mkdir -p out
case ${1:-} in
install) rm -f out/install.img out/software.img out/disk.img ;;
software) rm -f out/software.img out/disk.img ;;
setup) rm -f out/disk.img ;;
'') ;;
*) echo "usage: $0 [install|software|setup]" >&2; exit 1 ;;
esac

root=../../../..
# Copies a disk image as a copy-on-write clone where the file system can: APFS on macOS, Btrfs or XFS on Linux
clone() { cp -c "$1" "$2" 2>/dev/null || cp --reflink=auto "$1" "$2" 2>/dev/null || cp "$1" "$2"; }
target=i686-winxp-windows-gnu

# The guest integration in Rust with the Windows XP toolchain: the programs, then the drivers
xp=$root/tools/winxp-toolchain/bin
"$xp/cargo" build --release --locked --manifest-path guest/Cargo.toml --bin v86agent --bin v86setup --bin confid
"$xp/cargo-driver" build --release --locked --manifest-path guest/Cargo.toml --bin v86mou --bin v86tools --bin v86vid

# Software that is unpacked on the host
for dir in software/*/; do [ ! -f "$dir/Makefile" ] || make -s -C "$dir"; done

# The native apps of the repository, built with its Windows XP toolchain
"$xp/cargo" build --release --locked -p bastiaan-app -p reversi -p game2048
release=$root/target/$target/release
cp "$release/bastiaan-app.exe" software/bastiaan/bastiaan.exe
for app in reversi game2048; do cp "$release/$app.exe" "software/$app/"; done
# 2048's web view, which loads it from its own folder
build_dir=${CARGO_BUILD_BUILD_DIR:-$root/target/cargo-build/winxp}
cp "$build_dir/$target/release/miniblink.dll" software/game2048/

# RedSquare is built with NASM.
redsquare=$root/examples/windows/redsquare
make -C "$redsquare" redsquare-x86.exe
cp "$redsquare/redsquare-x86.exe" software/redsquare/redsquare.exe

# 1. Unattended Windows install in QEMU
if [ ! -f out/install.img ]; then
    nasm -f bin -o out/mbr.bin mbr.asm
    ./install.mjs winxp.iso out/mbr.bin out/install.part.img
    mv out/install.part.img out/install.img
fi

# From the CD: the Standard PC HAL (QEMU has an APIC, so setup installs the MPS HAL, which hangs on v86 without one)
# and MS Gothic, so Japanese names show up through font linking
if [ ! -f out/hal.dll ] || [ ! -f out/msgothic.ttc ]; then
    ./iso.mjs winxp.iso out I386/HAL.DL_ I386/LANG/MSGOTHIC.TT_
    bsdtar -xf out/HAL.DL_ -C out
    bsdtar -xf out/MSGOTHIC.TT_ -C out
    rm out/HAL.DL_ out/MSGOTHIC.TT_
fi

# 2. Software installs in QEMU, on a copy of the base install
if [ ! -f out/software.img ]; then
    ./winxp-dev-toolchain/build.sh
    rm -rf software/toolchain/Toolchain
    mkdir -p software/toolchain/Toolchain
    cp -R "$root/target/winxp-dev-toolchain/Toolchain/." software/toolchain/Toolchain/
    clone out/install.img out/software.part.img
    ./disk.mjs software out/software.part.img
    ./software.mjs out/software.part.img
    mv out/software.part.img out/software.img
fi

# 3. First v86 boot: drivers, settings, activation and the bplaat account, then clean up
if [ ! -f out/disk.img ]; then
    clone out/software.img out/disk.part.img
    ./disk.mjs setup out/disk.part.img
    ./setup.mjs out/disk.part.img
    ./disk.mjs finish out/disk.part.img
    mv out/disk.part.img out/disk.img
fi

# 4. The freshly built native apps over the installed ones, on a copy so the stages above stay as they are
clone out/disk.img out/site.img
./disk.mjs apps out/site.img

# 5. Streamed disk chunks, only the changed ones, the desktop snapshot and the boot screen for the site
./make-state.mjs out/site.img ../web/public/winxp
./bootscreen.mjs out/site.img ../web/public/winxp
