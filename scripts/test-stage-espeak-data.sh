#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
fail=0
check() { if ! eval "$1"; then echo "FAIL: $2" >&2; fail=1; fi; }

# A stand-in for `cargo build --message-format=json`: other packages first, then the sys crate's
# build script, then an unrelated later one.
out="$tmp/out"
mkdir -p "$out/share/espeak-ng-data/lang" "$tmp/bin"
: > "$out/share/espeak-ng-data/phontab"
: > "$out/share/espeak-ng-data/lang/ku"
cat > "$tmp/bin/cargo" <<SH
#!/usr/bin/env bash
echo "\$*" > "$tmp/cargo.args"
echo '{"reason":"build-script-executed","package_id":"registry+https://github.com/rust-lang/crates.io-index#cc@1.0.0","out_dir":"$tmp/wrong"}'
echo '{"reason":"build-script-executed","package_id":"registry+https://github.com/rust-lang/crates.io-index#dengjen-espeak-rs-sys@0.2.11","out_dir":"$out"}'
echo '{"reason":"build-finished","success":true}'
SH
chmod +x "$tmp/bin/cargo"
export PATH="$tmp/bin:$PATH"

./stage-espeak-data.sh "$tmp/dest" --release
check '[ -f "$tmp/dest/espeak-ng-data/phontab" ]' "stages the data from the sys crate's out_dir"
check '[ -f "$tmp/dest/espeak-ng-data/lang/ku" ]' "stages nested files"
check 'grep -q -- "--locked -p dengjen-espeak-phonemizer --features dengjen-espeak-rs-sys/compile-espeak-intonations" "$tmp/cargo.args"' "builds the phonemizer with the sys crate's data feature and a locked lockfile"
check 'grep -q -- "--release" "$tmp/cargo.args"' "passes extra args through to cargo"

: > "$tmp/dest/espeak-ng-data/stale"
./stage-espeak-data.sh "$tmp/dest"
check '[ ! -e "$tmp/dest/espeak-ng-data/stale" ]' "replaces a previous staging"

rm "$out/share/espeak-ng-data/phontab"
if ./stage-espeak-data.sh "$tmp/dest2" 2>"$tmp/err"; then
  echo "FAIL: succeeds without built data" >&2; fail=1
else
  check 'grep -q "no built espeak-ng-data" "$tmp/err"' "names the problem when the data was not built"
fi

# Git Bash lists Git's link.exe (in its usr/bin) before MSVC's; the staging must pick MSVC's.
gitbin="$tmp/gitbin"; msvc="$tmp/msvc"; mkdir -p "$gitbin" "$msvc" "$tmp/other"
: > "$gitbin/link.exe"
: > "$msvc/link.exe"
(
  # shellcheck source=/dev/null
  source ./stage-espeak-data.sh
  fail=0
  export GIT_BASH_USR_BIN="$gitbin"
  PATH="$gitbin:$tmp/other:$msvc:$PATH"
  check '[ "$(msvc_linker)" = "$msvc/link.exe" ]' "skips Git's link.exe and picks MSVC's"
  PATH="$gitbin:$tmp/other"
  check '! msvc_linker >/dev/null' "reports no linker when only Git's link.exe is on the PATH"
  exit "$fail"
) || fail=1

exit "$fail"
