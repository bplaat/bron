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

# Guest and software builds, cross-compiled with MinGW-w64
docker build -q -t winxp-tools . >/dev/null
# The project is mounted, the guest uses the site's avatar
make() { docker run --rm -v "$PWD/..:/w" -w "/w/winxp-image/$1" winxp-tools make; }
make guest
for dir in software/*/; do [ ! -f "$dir/Makefile" ] || make "$dir"; done

# The native apps of the repository, built with its Windows XP toolchain
root=../../../..
"$root/tools/winxp-toolchain/cargo" build --release --locked -p bastiaan -p reversi
for app in bastiaan reversi; do cp "$root/target/i686-winxp-windows-gnu/release/$app.exe" "software/$app/"; done

# 1. Unattended Windows install in QEMU
if [ ! -f out/install.img ]; then
    docker run --rm -v "$PWD/out:/out" winxp-tools cp /usr/lib/syslinux/mbr/mbr.bin /out/
    node install.mjs winxp.iso out/mbr.bin out/install.part.img
    mv out/install.part.img out/install.img
fi

# From the CD: the Standard PC HAL (QEMU has an APIC, so setup installs the MPS HAL, which hangs on v86 without one)
# and MS Gothic, so Japanese names show up through font linking
if [ ! -f out/hal.dll ] || [ ! -f out/msgothic.ttc ]; then
    docker run --rm -v "$PWD:/w" -w /w winxp-tools sh -c '7z e -y -oout winxp.iso I386/HAL.DL_ I386/LANG/MSGOTHIC.TT_ \
        >/dev/null && cabextract -q -d out out/HAL.DL_ out/MSGOTHIC.TT_ && rm out/HAL.DL_ out/MSGOTHIC.TT_'
fi

# 2. Software installs in QEMU, on a copy of the base install
if [ ! -f out/software.img ]; then
    cp -c out/install.img out/software.part.img 2>/dev/null || cp out/install.img out/software.part.img
    node disk.mjs software out/software.part.img
    node software.mjs out/software.part.img
    mv out/software.part.img out/software.img
fi

# 3. First v86 boot: drivers, settings, activation and the bplaat account, then clean up
if [ ! -f out/disk.img ]; then
    cp -c out/software.img out/disk.part.img 2>/dev/null || cp out/software.img out/disk.part.img
    node disk.mjs setup out/disk.part.img
    node setup.mjs out/disk.part.img
    node disk.mjs finish out/disk.part.img
    mv out/disk.part.img out/disk.img
fi

# 4. Streamed disk chunks, the desktop snapshot and the boot screen for the site
node make-state.mjs out/disk.img ../web/public/winxp
node bootscreen.mjs out/disk.img ../web/public/winxp
