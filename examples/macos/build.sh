#!/bin/sh
set -eu
cd "$(dirname "$0")"

case "${1:-assembly}" in
    assembly)
        for source in hello-*-x86_64.s; do
            nasm -f bin "$source" -o "${source%.s}"
            chmod +x "${source%.s}"
        done
        for source in hello*-arm64.s; do
            python3 build-arm64.py "$source"
        done
        nasm -f bin alert/asm/alert.s -o alert/asm/alert
        for source in portable-executable/hello.s portable-executable/alert.s; do
            python3 portable-executable/build-portable.py "$source"
        done
        ;;
    native)
        for project in alert/* window/* metal-triangle/* opengl-triangle/* metal-blocks/* opengl-blocks/*; do
            make -C "$project"
        done
        ;;
    *)
        echo "Usage: $0 [assembly|native]" >&2
        exit 1
        ;;
esac
