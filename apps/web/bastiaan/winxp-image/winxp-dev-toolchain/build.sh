#!/bin/sh
# Cross-builds the development tools that run inside 32-bit Windows XP to
# target/winxp-dev-toolchain/Toolchain/, with the C compiler of the Windows XP toolchain
set -eu
cd "$(dirname "$0")"
root=$(cd ../../../../.. && pwd)
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

# Edits a file with a sed script, in place on both GNU and BSD sed
edit() {
    sed "$1" "$2" > "$2.tmp"
    mv "$2.tmp" "$2"
}

source_archive() {
    name=$1
    shift
    download "$@"
    if [ ! -d "src/$name" ]; then
        mkdir -p "src/$name.part"
        tar -xf "downloads/$1" --strip-components=1 -C "src/$name.part"
        mv "src/$name.part" "src/$name"
    fi
}

source_archive tinycc tinycc-56b7510.tar.gz \
    https://codeload.github.com/gr8001/tinycc/tar.gz/56b75101ef323cca9e45c925ba17042ce87134de \
    c1927898f8c61e1781f77f4598699cf3777cc05f892ae6651eae69b3f5c57c1c
source_archive nasm nasm-2.16.03.tar.xz \
    https://www.nasm.us/pub/nasm/releasebuilds/2.16.03/nasm-2.16.03.tar.xz \
    1412a1c760bbd05db026b6c0d1657affd6631cd0a63cddb6f73cc6d4aa616148
source_archive binutils binutils-2.42.tar.xz \
    https://ftp.gnu.org/gnu/binutils/binutils-2.42.tar.xz \
    f6e4d41fd5fc778b06b7891457b3620da5ecea1006c6a4a41ae998109f85a800
source_archive make make-4.4.1.tar.gz \
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
edit 's/-c bcheck.c /-c $< /' src/tinycc/lib/Makefile
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
        --libpaths='{B}/../sysroot/lib' --tcc-switches='-DWINVER=0x0501 -D_WIN32_WINNT=0x0501'
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
# TinyCC's Win32 headers omit the COM property identifier used by GDI+.
edit 's/^typedef WORD UINT16;$/&\
typedef ULONG PROPID;/' Toolchain/sysroot/include/gdiplus/gdiplus.h
cp src/tinycc/win32/lib/*.def obj/tcc/libtcc1.a Toolchain/sysroot/lib/
# The installed GDI+ DLL exports undecorated names. TinyCC handles stdcall from the declarations.
sed 's/@[0-9][0-9]*$//' downloads/gdiplus-v10.0.0.def > Toolchain/sysroot/lib/gdiplus.def
find obj/tcc -maxdepth 1 -name '*.o' \( -name 'runmain.o' -o -name 'bcheck*.o' -o -name 'bt-*.o' \) \
    -exec cp {} Toolchain/sysroot/lib/ \;

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
edit 's/^#define DEFAULT_PREPROCESSOR_CMD "gcc"$/#define DEFAULT_PREPROCESSOR_CMD "tcc"/' \
    src/binutils/binutils/resrc.c
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
