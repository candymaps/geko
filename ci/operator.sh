#!/usr/bin/env bash
# The operator mode, checked in the kind cluster that `make start-dev` makes when `[deploy] mode` is
# "operator" (docs/OPERATOR_MODE_PLAN.md, step 8), once it is up: each module answers through the
# gateway; a module that falls does not take the other with it; and each scales on its own.
#
#   GEKO_DEV_FOLLOW=0 make start-dev DEV_CONFIG=deploy/overlays/ci/geko.toml && ci/operator.sh
#
# From the environment: GEKO_DEV_PORT (8080) and GEKO_DEV_CLUSTER (geko-dev), as for make start-dev.
set -euo pipefail
cd "$(dirname "$0")/.."

BASE=http://localhost:${GEKO_DEV_PORT:-8080}
CONTEXT=kind-${GEKO_DEV_CLUSTER:-geko-dev}
SEARCH='/geocoder/v1/search?text=Luxembourg'

k() { kubectl --context "$CONTEXT" -n geko "$@"; }
code() { curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$BASE$1" || true; }
fail() { echo "FAILED: $*" >&2; exit 1; }

# `what` is answered with the status `want` right now.
expect() {
    local what=$1 path=$2 want=$3 got
    got=$(code "$path")
    [ "$got" = "$want" ] || fail "$what: $path answered $got, not $want"
    echo "ok   $what"
}

# `path` is answered with `want` within `seconds`.
within() {
    local path=$1 want=$2 deadline=$(($(date +%s) + $3))
    until [ "$(code "$path")" = "$want" ]; do
        [ "$(date +%s)" -lt "$deadline" ] || return 1
        sleep 2
    done
}

# `make start-dev` leaves a port-forward behind, but a CI runner kills what a step left running when
# the step ends: open our own when nothing answers on the port.
if [ "$(code /)" = 000 ]; then
    k port-forward svc/geko-gateway "${BASE##*:}:80" > /dev/null &
    forward=$!
    trap 'kill "$forward" 2> /dev/null || true' EXIT
    for _ in $(seq 1 30); do
        [ "$(code /)" != 000 ] && break
        sleep 1
    done
    [ "$(code /)" != 000 ] || fail "the gateway does not answer on $BASE"
fi

echo "==> each module answers through the gateway"
expect "the geocoder searches" "$SEARCH" 200
expect "tiles serves its tilejson" /tiles/tilejson.json 200
curl -s --max-time 10 "$BASE$SEARCH" | grep -q '"features"' || fail "the search of the geocoder has no features"

echo "==> a pod of tiles falls: the geocoder goes on answering, and tiles comes back"
k delete pod -l app.kubernetes.io/component=tiles --wait=false > /dev/null
for _ in $(seq 1 15); do
    [ "$(code "$SEARCH")" = 200 ] || fail "the geocoder stopped answering while tiles was down"
    sleep 1
done
echo "ok   the geocoder answered all along"
within /tiles/tilejson.json 200 300 || fail "tiles did not come back within 300 s"
echo "ok   tiles is back"

echo "==> the geocoder scales to 3 and tiles stays at 1"
k scale deployment/geko-geocoder --replicas=3 > /dev/null
k rollout status deployment/geko-geocoder --timeout=300s
ready=$(k get deployment geko-geocoder -o jsonpath='{.status.readyReplicas}')
tiles=$(k get deployment geko-tiles -o jsonpath='{.spec.replicas}')
[ "$ready" = 3 ] || fail "the geocoder has $ready ready replicas, not 3"
[ "$tiles" = 1 ] || fail "tiles has $tiles replicas, not 1"
expect "the geocoder, with 3 replicas" "$SEARCH" 200
expect "tiles, with 1" /tiles/tilejson.json 200
echo "passed"
