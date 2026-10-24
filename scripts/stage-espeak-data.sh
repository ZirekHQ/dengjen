#!/usr/bin/env bash
# Builds the eSpeak-ng data from the espeak-ng commit that dengjen-espeak-rs-sys pins and copies it
# to <dest>/espeak-ng-data. The data is built from source, so it carries upstream fixes such as the
# Kurmanji word stress; the system's copy and the old committed one do not.
#   stage-espeak-data.sh <dest> [extra cargo build args]
set -euo pipefail

# Git for Windows puts its own link.exe (a coreutils tool) first on Git Bash's PATH, where it
# shadows MSVC's and breaks linking the build scripts.
msvc_linker() {
  local dir git_bin="${GIT_BASH_USR_BIN:-/usr/bin}"
  while IFS= read -r dir; do
    if [ "$dir" != "$git_bin" ] && [ -f "$dir/link.exe" ]; then
      echo "$dir/link.exe"
      return 0
    fi
  done < <(tr ':' '\n' <<<"$PATH")
  return 1
}

main() {
  local dest="${1:?dest}"
  shift
  local root linker out_dir data
  root="$(cd "$(dirname "$0")/.." && pwd)"

  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
      if linker="$(msvc_linker)"; then
        export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="$linker"
      fi
      ;;
  esac

  # The feature is enabled from the command line, not declared in a manifest: cargo-hakari would
  # otherwise unify it into every workspace build, including cross builds that cannot run the
  # espeak-ng binary the data build needs.
  out_dir="$(cd "$root" && cargo build --locked -p dengjen-espeak-phonemizer \
    --features dengjen-espeak-rs-sys/compile-espeak-intonations \
    --message-format=json-render-diagnostics "$@" \
    | jq -r 'select(.reason == "build-script-executed" and (.package_id | contains("dengjen-espeak-rs-sys"))) | .out_dir' \
    | tail -n1)"

  data="$out_dir/share/espeak-ng-data"
  if [ -z "$out_dir" ] || [ ! -f "$data/phontab" ]; then
    echo "stage-espeak-data: no built espeak-ng-data under '${out_dir:-<no out_dir>}'" >&2
    exit 1
  fi

  rm -rf "$dest/espeak-ng-data"
  mkdir -p "$dest"
  cp -R "$data" "$dest/espeak-ng-data"
  echo "staged espeak-ng-data into $dest"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
