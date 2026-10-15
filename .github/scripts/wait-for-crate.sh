#!/usr/bin/env bash
set -euo pipefail

name="$1"
version="$2"

# Poll the sparse index cargo actually resolves dependencies against, not the
# web API: a successful publish response doesn't guarantee the index has been
# updated yet (https://doc.rust-lang.org/cargo/reference/registry-web-api.html).
case "${#name}" in
  1) prefix="1" ;;
  2) prefix="2" ;;
  3) prefix="3/${name:0:1}" ;;
  *) prefix="${name:0:2}/${name:2:2}" ;;
esac
url="https://index.crates.io/${prefix}/${name}"
max_attempts=60
sleep_seconds=5

for attempt in $(seq 1 "$max_attempts"); do
  body=$(curl -s --connect-timeout 10 --max-time 30 \
    -H "User-Agent: dengjen-tts-publish-ci (https://github.com/ZirekHQ/dengjen)" \
    "$url") || body=""
  if printf '%s\n' "$body" | grep -q "\"vers\":\"${version}\""; then
    echo "${name} ${version} is live on the crates.io index"
    exit 0
  fi
  echo "Waiting for ${name} ${version} to appear on the crates.io index (attempt ${attempt}/${max_attempts})..."
  sleep "$sleep_seconds"
done

echo "Timed out waiting for ${name} ${version} to appear on the crates.io index" >&2
exit 1
