#!/usr/bin/env bash
# Tests scripts/central-deployment.sh against a stub curl that replays canned Central answers.
set -euo pipefail
script="$(cd "$(dirname "$0")" && pwd)/central-deployment.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

# The stub logs each call and answers /status with the next line of $work/states.
mkdir -p "$work/bin"
cat > "$work/bin/curl" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_DIR/calls"
case "$*" in
  *"/status?id="*)
    state="$(head -n1 "$STUB_DIR/states")"; sed -i 1d "$STUB_DIR/states"
    printf '{"deploymentState":"%s","errors":{}}\n' "$state" ;;
esac
STUB
chmod +x "$work/bin/curl"
run() {
  : > "$work/calls"
  PATH="$work/bin:$PATH" STUB_DIR="$work" RUNNER_TEMP="$work" CENTRAL_USERNAME=u CENTRAL_PASSWORD=p \
    CENTRAL_POLL_SECONDS=0 CENTRAL_POLL_ATTEMPTS=5 bash "$script" "$@"
}

printf 'PENDING\nVALIDATING\nVALIDATED\n' > "$work/states"
run wait abc-123 >/dev/null || fail "VALIDATED must pass"
grep -q DELETE "$work/calls" && fail "a validated deployment must not be dropped"

printf 'VALIDATING\nFAILED\n' > "$work/states"
if run wait abc-123 >/dev/null 2>&1; then fail "FAILED must fail"; fi
grep -q DELETE "$work/calls" && fail "a failed deployment stays in the portal"

printf 'PUBLISHING\n' > "$work/states"
run publish abc-123 >/dev/null || fail "publish must pass once PUBLISHING"
grep -q -- '-X POST .*/deployment/abc-123' "$work/calls" || fail "publish must POST the deployment"

printf 'VALIDATING\nVALIDATING\nVALIDATING\nVALIDATING\nVALIDATING\n' > "$work/states"
if run wait abc-123 >/dev/null 2>&1; then fail "a deployment stuck in VALIDATING must time out"; fi

run drop abc-123 >/dev/null || fail "drop must pass"
grep -q 'DELETE .*/deployment/abc-123' "$work/calls" || fail "drop must DELETE the deployment"

if run wait 'abc;rm' >/dev/null 2>&1; then fail "a malformed id must be refused"; fi
echo "central-deployment: ok"
