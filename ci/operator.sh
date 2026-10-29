#!/usr/bin/env bash
# The operator mode, checked in the kind cluster that `make start-dev` makes when `[deploy] mode` is
# "operator" (docs/OPERATOR_MODE_PLAN.md, step 10), once it is up: each module answers through the
# gateway; the dashboard lists them as the cluster runs them; a module that falls does not take the
# other with it; and the operator follows the config map: each module scales on its own, and one
# turned off is removed.
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

# The config map `geko` with the text of the file `$1` as its geko.toml: what the operator reads.
set_config() { k create configmap geko --from-file=geko.toml="$1" --dry-run=client -o yaml | k apply -f - > /dev/null; }

# The command `$@` succeeds within the `$1` seconds before it.
eventually() {
    local deadline=$(($(date +%s) + $1))
    shift
    until "$@" > /dev/null 2>&1; do
        [ "$(date +%s)" -lt "$deadline" ] || return 1
        sleep 2
    done
}

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

conf=$(mktemp)
edited=$(mktemp)
trap 'rm -f "$conf" "$edited"; kill "${forward:-}" 2> /dev/null || true' EXIT
k get configmap geko -o jsonpath='{.data.geko\.toml}' > "$conf"

echo "==> the dashboard lists the modules as the cluster runs them"
expect "the dashboard is served" / 200
listed=$(curl -s --max-time 10 "$BASE/admin/api/modules")
for module in geocoder tiles; do
    echo "$listed" | grep -q "\"name\":\"$module\"" || fail "the dashboard does not list $module: $listed"
done
echo "$listed" | grep -q '"unavailable":"' && fail "the dashboard says a module is unavailable: $listed"
echo "ok   both modules, available"

echo "==> the geocoder scales to 3 through the configuration and tiles stays at 1"
{ cat "$conf"; printf '\n[deploy.geocoder]\nreplicas = 3\n'; } > "$edited"
set_config "$edited"
eventually 120 test "$(k get deployment geko-geocoder -o jsonpath='{.spec.replicas}')" = 3 || fail "the operator did not scale the geocoder to 3"
k rollout status deployment/geko-geocoder --timeout=300s
ready=$(k get deployment geko-geocoder -o jsonpath='{.status.readyReplicas}')
tiles=$(k get deployment geko-tiles -o jsonpath='{.spec.replicas}')
[ "$ready" = 3 ] || fail "the geocoder has $ready ready replicas, not 3"
[ "$tiles" = 1 ] || fail "tiles has $tiles replicas, not 1"
expect "the geocoder, with 3 replicas" "$SEARCH" 200
expect "tiles, with 1" /tiles/tilejson.json 200

echo "==> by hand, the operator puts it back: kubectl scale does not stay"
k scale deployment/geko-geocoder --replicas=1 > /dev/null
eventually 120 test "$(k get deployment geko-geocoder -o jsonpath='{.spec.replicas}')" = 3 || fail "the operator did not restore the 3 replicas"
echo "ok   3 replicas again"

echo "==> tiles turned off in the configuration is removed, and the dashboard stops listing it"
sed 's/^tiles *= *true/tiles = false/' "$conf" > "$edited"
set_config "$edited"
eventually 120 test -z "$(k get deployment geko-tiles --ignore-not-found -o name)" || fail "the operator did not delete tiles"
eventually 60 sh -c "! curl -s --max-time 5 '$BASE/admin/api/modules' | grep -q '\"name\":\"tiles\"'" || fail "the dashboard still lists tiles"
[ -n "$(k get persistentvolumeclaim geko-data --ignore-not-found -o name)" ] || fail "the volume with the bundles went with tiles"
within "$SEARCH" 200 120 || fail "the geocoder stopped answering when tiles went"
echo "ok   tiles removed, the volume and the geocoder stay"

echo "==> tiles turned on again comes back with the bundle it had"
set_config "$conf"
within /tiles/tilejson.json 200 300 || fail "tiles did not come back within 300 s"
echo "ok   tiles is back"
echo "passed"
