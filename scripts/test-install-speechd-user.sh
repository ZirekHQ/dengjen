#!/usr/bin/env bash
# Tests install-speechd-user.sh against stand-in .deb files. Needs dpkg-deb; run as a normal user.
# shellcheck disable=SC2016 # check() evaluates its single-quoted conditions later.
set -euo pipefail
cd "$(dirname "$0")"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
fail=0
check() { if ! eval "$1"; then echo "FAIL: $2" >&2; fail=1; fi; }

case "$(uname -m)" in aarch64 | arm64) arch=arm64 ;; *) arch=amd64 ;; esac

# Builds a .deb from a staged tree with the given files.
make_deb() {
  local name="$1" root="$tmp/stage-$1"
  mkdir -p "$root/DEBIAN"
  printf 'Package: %s\nVersion: 9.9.9-1\nArchitecture: %s\nMaintainer: test <t@example.com>\nDescription: test\n' \
    "$name" "$arch" > "$root/DEBIAN/control"
  dpkg-deb --root-owner-group --build "$root" "$tmp/debs/${name}_9.9.9-1_$arch.deb" >/dev/null
}
mkdir -p "$tmp/debs"
sd="$tmp/stage-dengjen-speechd/usr/lib/speech-dispatcher-modules"
mkdir -p "$sd/espeak-ng-data" "$tmp/stage-dengjen-speechd/etc/speech-dispatcher/modules"
printf '#!/bin/sh\necho module\n' > "$sd/sd_dengjen"; chmod 755 "$sd/sd_dengjen"
: > "$sd/espeak-ng-data/phontab"
echo '# sd_dengjen has no module options.' > "$tmp/stage-dengjen-speechd/etc/speech-dispatcher/modules/dengjen.conf"
make_deb dengjen-speechd
cli="$tmp/stage-dengjen/usr/lib/dengjen"
mkdir -p "$cli/espeak-ng-data"
# The stand-in voice manager records how it was called.
printf '#!/bin/sh\necho "$*" >> "$HOME/cli.calls"\n' > "$cli/dengjen"; chmod 755 "$cli/dengjen"
: > "$cli/espeak-ng-data/phontab"
make_deb dengjen

run() { env -u XDG_CONFIG_HOME -u XDG_DATA_HOME HOME="$tmp/home" bash ./install-speechd-user.sh "$@"; }
home="$tmp/home"
mods="$home/.local/libexec/speech-dispatcher-modules"
mkdir -p "$home"

run --deb-dir "$tmp/debs" > "$tmp/out"
check '[ -x "$mods/sd_dengjen" ]' "installs the module where Speech Dispatcher looks for user modules"
check '[ -d "$home/.local/share" ]' "creates the data directory Speech Dispatcher resolves the module directory through"
check '[ -f "$mods/espeak-ng-data/phontab" ]' "installs the module's espeak-ng-data beside it"
check '[ -f "$home/.config/speech-dispatcher/modules/dengjen.conf" ]' "installs the module configuration"
check '[ "$(readlink "$home/.local/bin/dengjen")" = "$home/.local/lib/dengjen/dengjen" ]' "links the voice manager into ~/.local/bin"
check '[ -f "$home/.local/lib/dengjen/espeak-ng-data/phontab" ]' "installs the voice manager's espeak-ng-data beside it"
check 'grep -qx "voices install ku_TR-berfin_renas-medium" "$home/cli.calls"' "installs the Kurmanji voice by default"
check 'grep -q "pkill -f speech-dispatcher" "$tmp/out"' "says how to restart Speech Dispatcher"

: > "$mods/espeak-ng-data/stale"
rm "$home/cli.calls"
run --deb-dir "$tmp/debs" --no-voice > "$tmp/out"
check '[ ! -e "$mods/espeak-ng-data/stale" ]' "replaces the data of an earlier install"
check '[ ! -e "$home/cli.calls" ]' "skips the voice with --no-voice"

run --deb-dir "$tmp/debs" --voice en_GB-alba-low > /dev/null
check 'grep -qx "voices install en_GB-alba-low" "$home/cli.calls"' "installs the voice named by --voice"

printf 'AddModule "espeak-ng" "sd_espeak-ng" "espeak-ng.conf"\n' > "$home/.config/speech-dispatcher/speechd.conf"
run --deb-dir "$tmp/debs" --no-voice > "$tmp/out"
check 'grep -q "AddModule \"dengjen\"" "$tmp/out"' "warns when speechd.conf turns module detection off"
printf 'AddModule "dengjen" "sd_dengjen" "dengjen.conf"\n' >> "$home/.config/speech-dispatcher/speechd.conf"
run --deb-dir "$tmp/debs" --no-voice > "$tmp/out"
check '! grep -q "turns module detection off" "$tmp/out"' "does not warn once dengjen is listed"

mkdir -p "$home/.local/share/dengjen/voices/piper/x"
run --uninstall > /dev/null
check '[ ! -e "$mods/sd_dengjen" ] && [ ! -e "$mods/espeak-ng-data" ]' "uninstall removes the module"
check '[ ! -e "$home/.local/bin/dengjen" ] && [ ! -e "$home/.local/lib/dengjen" ]' "uninstall removes the voice manager"
check '[ ! -e "$home/.config/speech-dispatcher/modules/dengjen.conf" ]' "uninstall removes the configuration"
check '[ -d "$home/.local/share/dengjen/voices/piper/x" ]' "uninstall keeps downloaded voices"

# Speech Dispatcher derives the user module directory from XDG_DATA_HOME.
HOME="$tmp/home2" XDG_DATA_HOME="$tmp/home2/data" XDG_CONFIG_HOME="$tmp/home2/config" \
  bash ./install-speechd-user.sh --deb-dir "$tmp/debs" --no-voice > /dev/null
check '[ -x "$tmp/home2/libexec/speech-dispatcher-modules/sd_dengjen" ]' "follows XDG_DATA_HOME like Speech Dispatcher"
check '[ -f "$tmp/home2/config/speech-dispatcher/modules/dengjen.conf" ]' "follows XDG_CONFIG_HOME"

mkdir -p "$tmp/empty"
if run --deb-dir "$tmp/empty" > /dev/null 2> "$tmp/err"; then
  echo "FAIL: succeeds without the .deb files" >&2; fail=1
else
  check 'grep -q "needs dengjen-speechd_" "$tmp/err"' "names the missing .deb files"
fi

exit "$fail"
