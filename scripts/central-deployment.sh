#!/usr/bin/env bash
# Drives a USER_MANAGED deployment on the Maven Central Publisher API.
#
#   central-deployment.sh wait <deployment-id>     until VALIDATED; fails on FAILED
#   central-deployment.sh publish <deployment-id>  promotes it, then waits until PUBLISHING/PUBLISHED
#   central-deployment.sh drop <deployment-id>     deletes it; nothing reaches Maven Central
#
# Reads CENTRAL_USERNAME and CENTRAL_PASSWORD (a Central Portal user token). A staged deployment
# is the only reversible publish the release has, so release.yaml stages Java first and promotes
# it only after crates.io, PyPI and the Go and gRPC releases went out.
set -euo pipefail
action="${1:?usage: central-deployment.sh wait|publish|drop <deployment-id>}"
id="${2:?usage: central-deployment.sh wait|publish|drop <deployment-id>}"
api="${CENTRAL_API:-https://central.sonatype.com/api/v1/publisher}"
attempts="${CENTRAL_POLL_ATTEMPTS:-90}"
interval="${CENTRAL_POLL_SECONDS:-20}"
[[ "$id" =~ ^[A-Za-z0-9-]+$ ]] || { echo "::error::'$id' is not a deployment id" >&2; exit 1; }
auth="Authorization: Bearer $(printf '%s:%s' "${CENTRAL_USERNAME:?}" "${CENTRAL_PASSWORD:?}" | base64 | tr -d '\n')"

state() {
  curl -fsS -X POST -H "$auth" "${api}/status?id=${id}" > "${RUNNER_TEMP:-/tmp}/central-status.json"
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("deploymentState", ""))' \
    "${RUNNER_TEMP:-/tmp}/central-status.json"
}

drop() { curl -fsS -X DELETE -H "$auth" "${api}/deployment/${id}" > /dev/null; }

# Polls until the state is one of the arguments; FAILED always stops it.
wait_for() {
  local current
  for _ in $(seq "$attempts"); do
    current="$(state)"
    echo "deployment ${id}: ${current}"
    for want in "$@"; do [ "$current" = "$want" ] && return 0; done
    if [ "$current" = "FAILED" ]; then
      echo "::error::Maven Central refused deployment ${id}:" >&2
      cat "${RUNNER_TEMP:-/tmp}/central-status.json" >&2
      return 1
    fi
    sleep "$interval"
  done
  echo "::error::deployment ${id} did not reach $* in time" >&2
  return 1
}

case "$action" in
  # A FAILED deployment is left in place: the portal shows why it failed, and Sonatype asks for
  # it to be kept when support has to look. It never reaches Maven Central either way.
  wait) wait_for VALIDATED ;;
  publish)
    curl -fsS -X POST -H "$auth" "${api}/deployment/${id}" > /dev/null
    wait_for PUBLISHING PUBLISHED
    ;;
  drop)
    drop
    echo "dropped deployment ${id}"
    ;;
  *) echo "::error::unknown action '$action'" >&2; exit 1 ;;
esac
