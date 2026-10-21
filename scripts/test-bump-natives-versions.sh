#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'find "$work" -mindepth 1 -delete; rmdir "$work"' EXIT

# A throwaway repo with a stub fingerprint script: `--update` writes whatever STUB_TTS/STUB_STT hold.
init_repo() {
  repo="$(mktemp -d "$work/repo.XXXXXX")"
  mkdir -p "$repo/scripts" "$repo/bindings/java"
  cd "$repo"
  git init -q -b main
  git config user.email t@example.com
  git config user.name t
  cp "$here/bump-natives-versions.sh" scripts/
  cat > scripts/natives-fingerprint.sh <<'STUB'
#!/usr/bin/env bash
case "${NATIVES_KIND:-tts}" in
  tts) echo "${STUB_TTS:-old-tts}" > bindings/java/natives.fingerprint ;;
  stt) echo "${STUB_STT:-old-stt}" > bindings/java/stt-natives.fingerprint ;;
esac
STUB
  chmod +x scripts/*.sh
  echo old-tts > bindings/java/natives.fingerprint
  echo old-stt > bindings/java/stt-natives.fingerprint
  printf 'val nativesVersion = "1.0.9"\nval sttNativesVersion = "2.3.4"\n' > bindings/java/build.gradle.kts
  git add -A
  git commit -q -m base
}

version() { grep -m1 "^val $1 = " bindings/java/build.gradle.kts | sed -E 's/.*"([^"]+)".*/\1/'; }

fail() { echo "FAIL: $*"; exit 1; }

init_repo
scripts/bump-natives-versions.sh main
[ "$(version nativesVersion)" = 1.0.9 ] || fail "unchanged fingerprint must not bump tts"
[ "$(version sttNativesVersion)" = 2.3.4 ] || fail "unchanged fingerprint must not bump stt"

init_repo
STUB_TTS=new-tts scripts/bump-natives-versions.sh main
[ "$(version nativesVersion)" = 1.0.10 ] || fail "tts patch should go 1.0.9 -> 1.0.10, got $(version nativesVersion)"
[ "$(version sttNativesVersion)" = 2.3.4 ] || fail "stt must stay when only tts changed"

init_repo
STUB_STT=new-stt scripts/bump-natives-versions.sh main
[ "$(version sttNativesVersion)" = 2.3.5 ] || fail "stt patch should go 2.3.4 -> 2.3.5, got $(version sttNativesVersion)"

init_repo
sed -i 's/1.0.9/1.0.12/' bindings/java/build.gradle.kts
STUB_TTS=new-tts scripts/bump-natives-versions.sh main
[ "$(version nativesVersion)" = 1.0.12 ] || fail "an already bumped version must not be bumped again"

echo ok
