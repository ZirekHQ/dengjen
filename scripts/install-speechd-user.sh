#!/usr/bin/env bash
# Installs the dengjen Speech Dispatcher module and the `dengjen` voice manager for the current
# user, without root. Works on any glibc Linux distribution, not only Debian and Ubuntu: it unpacks
# the release .deb files into the home directory instead of installing them.
#
#   install-speechd-user.sh [--version X.Y.Z | --deb-dir DIR] [--voice KEY | --no-voice]
#   install-speechd-user.sh --uninstall
#
# Files it writes:
#   ~/.local/libexec/speech-dispatcher-modules/sd_dengjen (+ espeak-ng-data beside it)
#   ~/.local/lib/dengjen/dengjen (+ espeak-ng-data), linked from ~/.local/bin/dengjen
#   ~/.config/speech-dispatcher/modules/dengjen.conf
set -euo pipefail

REPO="ZirekHQ/dengjen"
DEFAULT_VOICE="ku_TR-berfin_renas-medium"

data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
# Speech Dispatcher looks for user modules in "$XDG_DATA_HOME/../libexec", which is
# ~/.local/libexec by default. It opens that path as written, so the data directory must exist.
module_dir="$data_home/../libexec/speech-dispatcher-modules"
lib_dir="$HOME/.local/lib/dengjen"
bin_dir="$HOME/.local/bin"
conf_dir="$config_home/speech-dispatcher/modules"

say() { printf '%s\n' "$*"; }
die() { printf 'install-speechd-user: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
}

deb_arch() {
  case "$(uname -m)" in
    x86_64 | amd64) echo amd64 ;;
    aarch64 | arm64) echo arm64 ;;
    *) die "no release build for $(uname -m); build from source instead (see docs/modules/ROOT/pages/speech-dispatcher.adoc)" ;;
  esac
}

fetch() {
  if command -v curl >/dev/null; then
    curl -fsSL --retry 3 -o "$2" "$1"
  elif command -v wget >/dev/null; then
    wget -q -O "$2" "$1"
  else
    die "needs curl or wget to download the release"
  fi
}

# The repository also publishes java-v* and grpc-release-v* releases, so "latest" is not enough.
latest_version() {
  local list="$1/releases.json"
  fetch "https://api.github.com/repos/$REPO/releases?per_page=100" "$list"
  grep -o '"tag_name": *"v[0-9]*\.[0-9]*\.[0-9]*"' "$list" | head -n 1 | grep -o '[0-9][0-9.]*' \
    || die "found no vX.Y.Z release of $REPO"
}

# Unpacks a .deb into a directory with whichever tool the system has.
unpack_deb() {
  local deb="$1" dest="$2"
  mkdir -p "$dest"
  if command -v dpkg-deb >/dev/null; then
    dpkg-deb -x "$deb" "$dest"
  elif command -v bsdtar >/dev/null; then
    bsdtar -xOf "$deb" 'data.tar*' | bsdtar -xf - -C "$dest"
  elif command -v ar >/dev/null; then
    # Inside $tmp, so the EXIT trap removes it even when tar fails.
    local work
    work="$tmp/ar-$(basename "$deb")"
    mkdir -p "$work"
    (cd "$work" && ar x "$(realpath "$deb")")
    if [ -e "$work/data.tar.xz" ] && ! command -v xz >/dev/null; then
      die "needs xz to unpack $(basename "$deb") with ar; install xz-utils or dpkg-deb"
    fi
    tar -xf "$work"/data.tar* -C "$dest"
  else
    die "needs dpkg-deb, bsdtar, or ar (binutils) with xz to unpack the .deb files"
  fi
}

# Replaces a directory in one step so a failed copy never leaves half of the old one.
replace_dir() {
  local src="$1" dest="$2"
  rm -rf "$dest.new"
  cp -R "$src" "$dest.new"
  rm -rf "$dest"
  mv "$dest.new" "$dest"
}

uninstall() {
  rm -f "$module_dir/sd_dengjen" "$conf_dir/dengjen.conf"
  rm -rf "$module_dir/espeak-ng-data" "$lib_dir"
  if [ -L "$bin_dir/dengjen" ]; then rm -f "$bin_dir/dengjen"; fi
  say "Removed the dengjen module and voice manager."
  say "Voices in $data_home/dengjen/voices were kept; delete that directory to remove them."
  say "Restart Speech Dispatcher: pkill -f speech-dispatcher (it starts again on the next request)."
}

warn_about_explicit_modules() {
  # The server reads the user's speechd.conf when it exists, and the system one otherwise.
  local conf="$config_home/speech-dispatcher/speechd.conf"
  [ -f "$conf" ] || conf="${SPEECHD_SYSTEM_CONF:-/etc/speech-dispatcher/speechd.conf}"
  if [ -f "$conf" ] && grep -q '^[[:space:]]*AddModule' "$conf" \
    && ! grep -q '^[[:space:]]*AddModule[[:space:]]*"dengjen"' "$conf"; then
    say ""
    say "Note: $conf lists its modules with AddModule, which turns module detection off."
    say "Add this line next to the others, or Speech Dispatcher will not load dengjen:"
    say '  AddModule "dengjen" "sd_dengjen" "dengjen.conf"'
  fi
}

main() {
  local version="" deb_dir="" voice="$DEFAULT_VOICE" action=install
  while [ $# -gt 0 ]; do
    case "$1" in
      --version) version="${2:?--version needs a value}"; shift 2 ;;
      --deb-dir) deb_dir="${2:?--deb-dir needs a value}"; shift 2 ;;
      --voice) voice="${2:?--voice needs a value}"; shift 2 ;;
      --no-voice) voice=""; shift ;;
      --uninstall) action=uninstall; shift ;;
      -h | --help) usage; return ;;
      *) usage >&2; die "unknown option $1" ;;
    esac
  done
  # Root would also make the module paths resolve under /, where the system package lives.
  [ "$(id -u)" -ne 0 ] || die "run this as the user who will hear the voice, not as root"
  if [ "$action" = uninstall ]; then
    uninstall
    return
  fi

  # Global, so the EXIT trap still sees it after main returns.
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  local arch speechd_deb cli_deb
  arch="$(deb_arch)"
  if [ -n "$deb_dir" ]; then
    speechd_deb="$(find "$deb_dir/" -maxdepth 1 -name "dengjen-speechd_*_$arch.deb" | sort | tail -n 1)"
    cli_deb="$(find "$deb_dir/" -maxdepth 1 -name "dengjen_*_$arch.deb" | sort | tail -n 1)"
    [ -n "$speechd_deb" ] && [ -n "$cli_deb" ] \
      || die "$deb_dir needs dengjen-speechd_*_$arch.deb and dengjen_*_$arch.deb"
  else
    [ -n "$version" ] || version="$(latest_version "$tmp")"
    say "Downloading dengjen $version for $arch..."
    local base="https://github.com/$REPO/releases/download/v$version"
    speechd_deb="$tmp/dengjen-speechd.deb"
    cli_deb="$tmp/dengjen.deb"
    fetch "$base/dengjen-speechd_${version}-1_$arch.deb" "$speechd_deb"
    fetch "$base/dengjen_${version}-1_$arch.deb" "$cli_deb"
  fi

  unpack_deb "$speechd_deb" "$tmp/speechd"
  unpack_deb "$cli_deb" "$tmp/cli"
  local sd_src="$tmp/speechd/usr/lib/speech-dispatcher-modules"
  [ -x "$sd_src/sd_dengjen" ] || die "$speechd_deb has no sd_dengjen"
  [ -x "$tmp/cli/usr/lib/dengjen/dengjen" ] || die "$cli_deb has no dengjen binary"

  mkdir -p "$data_home" "$module_dir" "$conf_dir" "$bin_dir" "$(dirname "$lib_dir")"
  # Both binaries look for espeak-ng-data beside themselves.
  install -m 755 "$sd_src/sd_dengjen" "$module_dir/sd_dengjen.new"
  mv "$module_dir/sd_dengjen.new" "$module_dir/sd_dengjen"
  replace_dir "$sd_src/espeak-ng-data" "$module_dir/espeak-ng-data"
  replace_dir "$tmp/cli/usr/lib/dengjen" "$lib_dir"
  ln -sfn "$lib_dir/dengjen" "$bin_dir/dengjen"
  install -m 644 "$tmp/speechd/etc/speech-dispatcher/modules/dengjen.conf" "$conf_dir/dengjen.conf"
  say "Installed sd_dengjen into $module_dir"
  say "Installed the voice manager as $bin_dir/dengjen"

  if [ -n "$voice" ]; then
    say "Installing the voice $voice..."
    "$lib_dir/dengjen" voices install "$voice"
  fi

  warn_about_explicit_modules
  say ""
  say "Next: restart Speech Dispatcher with 'pkill -f speech-dispatcher' (it starts again on the"
  say "next request), restart Orca, then check with: spd-say -o dengjen -l ku \"Silav\""
  case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) say "Add $bin_dir to your PATH to run 'dengjen voices' for more voices." ;;
  esac
}

main "$@"
