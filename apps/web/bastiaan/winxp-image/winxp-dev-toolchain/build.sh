#!/bin/sh
# Cross-builds the development tools that run inside 32-bit Windows XP to
# target/winxp-dev-toolchain/Toolchain/, with the C compiler of the Windows XP toolchain
set -eu
cd "$(dirname "$0")"
root=$(cd ../../../../.. && pwd)
headers=$(pwd)/include
patches=$(pwd)/patches
guids=$(pwd)/lib
xp=$root/tools/winxp-toolchain/bin
sysroot=$root/target/winxp-toolchain/sysroot
. "$root/tools/winxp-toolchain/llvm.sh"
"$root/tools/winxp-toolchain/build.sh"
mkdir -p "$root/target/winxp-dev-toolchain"
cd "$root/target/winxp-dev-toolchain"
# TinyCC puts the commit of a git repository in its version, the sources aren't in one
export GIT_CEILING_DIRECTORIES="$root/target"

triple=i686-w64-mingw32
jobs=$(getconf _NPROCESSORS_ONLN)
export CC="$xp/cc" AR="$xp/ar" RANLIB="${llvm}llvm-ranlib" WINDRES="$xp/windres" STRIP="${llvm}llvm-strip"
export CFLAGS='-O2 -DWINVER=0x0501 -D_WIN32_WINNT=0x0501'
export LDFLAGS='-static'
# The linker of bin/cc, which libtool must see as GNU compatible, else it converts paths with cygpath
export LD="$lld"
rm -rf Toolchain
mkdir -p downloads src obj Toolchain/bin Toolchain/include Toolchain/sysroot/include Toolchain/sysroot/lib

download() {
    archive=$1 url=$2 checksum=$3
    if [ ! -f "downloads/$archive" ]; then
        curl --fail --location --silent --show-error --retry 2 "$url" -o "downloads/$archive.part"
        mv "downloads/$archive.part" "downloads/$archive"
    fi
    printf '%s  %s\n' "$checksum" "downloads/$archive" | sha256sum -c - >/dev/null
}

# Unpacks a source archive to src/<name> with the patches of patches/<name>. When the patches change, it unpacks
# again and drops the builds in obj/ that the name prefixes, an unpacked file is older than its old object.
source_archive() {
    name=$1 builds=$2
    shift 2
    download "$@"
    stamp=$(cat "$patches/$name"/*.patch 2>/dev/null | sha256sum | cut -c1-64)
    if [ -d "src/$name" ] && [ "$(cat "src/$name.patches" 2>/dev/null)" != "$stamp" ]; then
        rm -rf "src/$name" obj/"$builds"*
    fi
    if [ ! -d "src/$name" ]; then
        rm -rf "src/$name.part"
        mkdir -p "src/$name.part"
        tar -xf "downloads/$1" --strip-components=1 -C "src/$name.part"
        for patch in "$patches/$name"/*.patch; do
            [ ! -f "$patch" ] || patch -s -p1 -d "src/$name.part" < "$patch"
        done
        mv "src/$name.part" "src/$name"
        echo "$stamp" > "src/$name.patches"
    fi
}

source_archive tinycc tcc tinycc-56b7510.tar.gz \
    https://codeload.github.com/gr8001/tinycc/tar.gz/56b75101ef323cca9e45c925ba17042ce87134de \
    c1927898f8c61e1781f77f4598699cf3777cc05f892ae6651eae69b3f5c57c1c
source_archive nasm nasm nasm-2.16.03.tar.xz \
    https://www.nasm.us/pub/nasm/releasebuilds/2.16.03/nasm-2.16.03.tar.xz \
    1412a1c760bbd05db026b6c0d1657affd6631cd0a63cddb6f73cc6d4aa616148
source_archive binutils binutils binutils-2.42.tar.xz \
    https://ftp.gnu.org/gnu/binutils/binutils-2.42.tar.xz \
    f6e4d41fd5fc778b06b7891457b3620da5ecea1006c6a4a41ae998109f85a800
source_archive make make make-4.4.1.tar.gz \
    https://ftp.gnu.org/gnu/make/make-4.4.1.tar.gz \
    dd16fb1d67bfab79a72f5e8390735c49e3e8e70b4945a15ab1f81ddb78658fb3
download gdiplus-v10.0.0.def \
    https://raw.githubusercontent.com/mingw-w64/mingw-w64/v10.0.0/mingw-w64-crt/lib32/gdiplus.def \
    8d01da8b938b5dc4715650fb840c714c672bcf6e2e245897a73949f99536549e

configure() {
    [ -f Makefile ] || "$@"
}

# Bootstrap the ELF objects for TinyCC's Win32 runtime with an XP-targeting TinyCC
# that runs on the build machine. The installed compiler itself is a Win32 PE.
mkdir -p obj/tcc-bootstrap obj/tcc
(
    cd obj/tcc-bootstrap
    unset CC AR RANLIB WINDRES STRIP CFLAGS LDFLAGS
    configure ../../src/tinycc/configure
    make -j"$jobs" i386-win32-tcc
)
(
    cd obj/tcc
    ../../src/tinycc/configure --cpu=i386 --targetos=WIN32 --cc="$CC" --ar="$AR" \
        --enable-static --sysincludepaths='{B}/../include;{B}/../sysroot/include;{B}/../sysroot/include/winapi' \
        --libpaths='{B}/../sysroot/lib'
    make CC=cc c2str.exe
    make -j"$jobs" tcc.exe
    make -j"$jobs" -C lib XTCC="$PWD/../tcc-bootstrap/i386-win32-tcc"
)
install -m 755 obj/tcc/tcc.exe Toolchain/bin/
cp src/tinycc/include/*.h src/tinycc/tcclib.h Toolchain/include/
cp -R src/tinycc/win32/include/. Toolchain/sysroot/include/
mkdir -p Toolchain/sysroot/include/gdiplus
install -m 644 "$sysroot/include/gdiplus.h" Toolchain/sysroot/include/
install -m 644 "$sysroot/include/gdiplus/"*.h Toolchain/sysroot/include/gdiplus/
# COM, DirectDraw, Direct3D 8, common controls and dialogs and multimedia from the mingw-w64 headers, TinyCC's own
# Windows headers stop at GDI. include/ has the COM base for C, patches/mingw-w64 what TinyCC's headers lack for them.
for header in _mingw_unicode.h _mingw_dxhelper.h apiset.h apisetcconv.h prsht.h commctrl.h commdlg.h \
    mmsystem.h mmsyscom.h mciapi.h mmiscapi.h mmiscapi2.h playsoundapi.h mmeapi.h timeapi.h joystickapi.h \
    ddraw.h d3d8.h d3d8types.h d3d8caps.h; do
    install -m 644 "$sysroot/include/$header" Toolchain/sysroot/include/
done
install -m 644 "$headers/"*.h Toolchain/sysroot/include/
for patch in "$patches/mingw-w64"/*.patch; do
    patch -s -p1 -d Toolchain/sysroot/include < "$patch"
done
cp src/tinycc/win32/lib/*.def obj/tcc/libtcc1.a Toolchain/sysroot/lib/
# Import definitions of the system DLLs from mingw-w64, undecorated like TinyCC's own
for library in advapi32 comctl32 comdlg32 d3d8 ddraw msimg32 ole32 oleaut32 shell32 winmm; do
    sed -e 's/;.*//' -e 's/@[0-9][0-9]*//' -e 's/[[:space:]]*$//' -e '/^$/d' \
        "$root/target/winxp-toolchain/src/mingw-w64/mingw-w64-crt/lib32/$library.def" > "Toolchain/sysroot/lib/$library.def"
done
# The installed GDI+ DLL exports undecorated names. TinyCC handles stdcall from the declarations.
sed 's/@[0-9][0-9]*$//' downloads/gdiplus-v10.0.0.def > Toolchain/sysroot/lib/gdiplus.def
find obj/tcc -maxdepth 1 -name '*.o' \( -name 'runmain.o' -o -name 'bcheck*.o' -o -name 'bt-*.o' \) \
    -exec cp {} Toolchain/sysroot/lib/ \;
# The interface IDs as libraries, built by TinyCC with the kit's headers
for library in uuid dxguid; do
    obj/tcc-bootstrap/i386-win32-tcc -nostdinc -IToolchain/include -IToolchain/sysroot/include \
        -IToolchain/sysroot/include/winapi -c "$guids/$library.c" -o "obj/tcc/$library.o"
    obj/tcc-bootstrap/i386-win32-tcc -ar rcs "Toolchain/sysroot/lib/lib$library.a" "obj/tcc/$library.o"
done

# The build machine, for the autotools cross builds
build=$(sh src/binutils/config.guess)
mkdir -p obj/nasm obj/binutils obj/make
(
    cd obj/nasm
    configure ../../src/nasm/configure --build="$build" --host="$triple"
    make dirs
    cp ../../src/nasm/win/manifest.rc win/
    make -j"$jobs" nasm.exe
)
(
    cd obj/binutils
    configure ../../src/binutils/configure --build="$build" --host="$triple" --target="$triple" \
        --disable-nls --disable-gdb --disable-gdbserver --disable-sim --disable-gprof --disable-gprofng \
        --disable-gas --disable-ld --disable-gold --disable-plugins --disable-libctf --without-zstd
    # Without info documentation
    make -j"$jobs" all-binutils MAKEINFO=true
)
(
    cd obj/make
    configure ../../src/make/configure --build="$build" --host="$triple" --disable-nls --without-guile
    make -j"$jobs"
)
install -m 755 obj/nasm/nasm.exe obj/binutils/binutils/windres.exe obj/make/make.exe Toolchain/bin/
"$STRIP" Toolchain/bin/*.exe
