#!/usr/bin/env bash
# Bundles the subset of the espeak-ng submodule that's needed to build this
# crate into a single xz-compressed tar (bundled/espeak-ng.tar.xz).
#
# crates.io caps a published .crate at 10MiB compressed; the raw espeak-ng
# tree (dictsource/phsource/espeak-ng-data/src/...) alone gzips to ~12.7MiB.
# xz gets the same content down to ~7.7MiB. cargo's own .crate container is
# hardcoded to gzip (no way to make `cargo publish` emit xz -- see
# https://github.com/rust-lang/cargo/issues/2526), so instead this bundle is
# pre-compressed with xz ourselves and shipped as one opaque file; build.rs
# decompresses it at build time. gzip re-wrapping an already-xz-compressed
# blob costs ~nothing, so the final .crate lands near the 7.7MiB figure.
#
# Not committed to git (crates/espeak-rs-sys/.gitignore) -- regenerated here
# from the submodule, matching this repo's preference for building release
# artifacts at release time rather than vendoring generated output.
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

# Same subset Cargo.toml's `include` used to list directly (dictsource minus
# its oversized `extra/` dir, minus the unused fo_list and Windows debug
# .wav asset) -- this script is now the single place that decision lives.
# No leading "./": tar matches --exclude against the names as recorded in
# the archive, which start with the argument names given below (verified;
# a "./"-prefixed pattern silently fails to match and the exclude is a
# no-op -- caught by scripts/bundle-espeak-ng.sh's own size check below).
tar -C espeak-ng \
    --exclude='dictsource/extra' \
    --exclude='dictsource/fo_list' \
    --exclude='src/windows/Debug/*.wav' \
    -cf "$tmp_tar" \
    CMakeLists.txt vim cmake phsource tests src espeak-ng-data dictsource espeak-ng.pc.in

# Re-add dictsource/extra/ru_listx, dropped by the blanket dictsource/extra
# exclude above: data.cmake wires it into the Russian dictionary build
# (EXTRA_ru, default ON) whenever compile-espeak-intonations is enabled, so
# dropping it silently degrades Russian pronunciation. See issue #23.
tar -C espeak-ng -rf "$tmp_tar" dictsource/extra/ru_listx

xz -9e -T0 -f -c "$tmp_tar" > bundled/espeak-ng.tar.xz

bundle_bytes=$(wc -c < bundled/espeak-ng.tar.xz)
echo "wrote $(du -h bundled/espeak-ng.tar.xz | cut -f1) bundled/espeak-ng.tar.xz"

# crates.io's .crate cap is 10MiB (10485760 bytes); leave headroom for the
# few small files Cargo.toml's `include` adds on top of this bundle
# (src/lib.rs, build.rs, wrapper.h) plus room to grow before hitting the
# wall again. Catches a regression like a broken --exclude silently
# including dictsource/extra/ back in (once did, in review -- it isn't
# obvious from the tar output alone).
max_bytes=9437184 # 9MiB
if [ "$bundle_bytes" -gt "$max_bytes" ]; then
    echo "error: bundle is $bundle_bytes bytes, over the $max_bytes byte budget" >&2
    exit 1
fi
