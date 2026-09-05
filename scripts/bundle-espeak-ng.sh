#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate_dir="$(dirname "$script_dir")"
cd "$crate_dir"

if [ ! -f espeak-ng/CMakeLists.txt ]; then
    echo "error: espeak-ng/CMakeLists.txt not found -- run 'git submodule update --init' first" >&2
    exit 1
fi

mkdir -p bundled
tmp_tar="$(mktemp)"
trap 'rm -f "$tmp_tar"' EXIT

tar -C espeak-ng \
    --exclude='dictsource/extra' \
    --exclude='dictsource/fo_list' \
    --exclude='src/windows/Debug/*.wav' \
    -cf "$tmp_tar" \
    CMakeLists.txt vim cmake phsource tests src espeak-ng-data dictsource espeak-ng.pc.in

tar -C espeak-ng -rf "$tmp_tar" dictsource/extra/ru_listx

xz -9e -T0 -f -c "$tmp_tar" > bundled/espeak-ng.tar.xz

bundle_bytes=$(wc -c < bundled/espeak-ng.tar.xz)
echo "wrote $(du -h bundled/espeak-ng.tar.xz | cut -f1) bundled/espeak-ng.tar.xz"

max_bytes=9437184 
if [ "$bundle_bytes" -gt "$max_bytes" ]; then
    echo "error: bundle is $bundle_bytes bytes, over the $max_bytes byte budget" >&2
    exit 1
fi
