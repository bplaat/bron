# Finds LLVM and lld, sourced by build.sh and bin/: Homebrew's on macOS, the newest
# /usr/lib/llvm-<version> of the distribution or apt.llvm.org on Linux, else the ones on the PATH.
# Sets llvm to the directory of the LLVM tools with a trailing slash, or empty, and lld to ld.lld.
llvm=''
for llvm_dir in /opt/homebrew/opt/llvm/bin/ /usr/local/opt/llvm/bin/ \
    $(ls -d /usr/lib/llvm-*/bin/ 2>/dev/null | sort -t- -k2 -n -r); do
    [ -x "${llvm_dir}clang" ] && llvm=$llvm_dir && break
done
lld=ld.lld
for llvm_dir in /opt/homebrew/opt/lld/bin/ /usr/local/opt/lld/bin/ "$llvm"; do
    [ -x "${llvm_dir}ld.lld" ] && lld="${llvm_dir}ld.lld" && break
done
unset llvm_dir
