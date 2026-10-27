#!/usr/bin/env bash
# What `make start-dev` and `make stop-dev` do when `[deploy] mode` of the configuration is
# "operator" (docs/OPERATOR_MODE_PLAN.md, step 8): the same manifests as a cluster, in a local one
# (kind), from the same configuration, behind the same URL as the single mode.
#
#   develop/k8s-dev.sh check             that docker, kind and kubectl are there, and docker answers
#   develop/k8s-dev.sh up [--no-follow]  create the cluster if there is none, load the image geko:dev
#                                        (which `make start-dev` has just built), apply the manifests
#                                        rendered from the configuration, build each module one after
#                                        the other, wait for the pods and serve it all on
#                                        http://localhost:$GEKO_DEV_PORT/ (then follow the logs of the
#                                        pods; Ctrl-C leaves it running)
#   develop/k8s-dev.sh down [--purge]    delete what runs but keep the cluster and its volume with the
#                                        bundles and downloads; --purge deletes the cluster too
#
# From the environment (make start-dev sets them): GEKO_DEV_CONFIG (the geko.toml), GEKO_DEV_MODULES
# (comma-separated, in the order they build), GEKO_DEV_PORT, GEKO_DEV_CLUSTER (default geko-dev),
# GEKO_DEV_FOLLOW=0 (as --no-follow) and GEKO_DEV_BUILD_WAIT (seconds one build may take, default 3600).
set -euo pipefail
cd "$(dirname "$0")/.."

CLUSTER=${GEKO_DEV_CLUSTER:-geko-dev}
CONTEXT=kind-$CLUSTER
NAMESPACE=geko
IMAGE=geko:dev
PORT=${GEKO_DEV_PORT:-8080}
CONFIG=${GEKO_DEV_CONFIG:-develop/geko.toml}
MODULES=${GEKO_DEV_MODULES:-geocoder,tiles}
BUILD_WAIT=${GEKO_DEV_BUILD_WAIT:-3600}
# The overlay of this run: the local one, with the manifests rendered from CONFIG and CONFIG itself
# as the config map. Generated, never committed.
OVERLAY=deploy/overlays/.dev

# kubectl for this cluster, whatever the current context of the machine is.
kubectl() { command kubectl --context "$CONTEXT" "$@"; }

check() {
    local missing=0
    need() { type -P "$1" > /dev/null || { echo "$1 is not installed: $2" >&2; missing=1; }; }
    need docker "Docker Desktop, or Docker Engine (https://docs.docker.com/get-docker/)"
    need kind "https://kind.sigs.k8s.io/docs/user/quick-start/#installation"
    need kubectl "https://kubernetes.io/docs/tasks/tools/"
    [ "$missing" = 0 ] || exit 1
    docker info > /dev/null 2>&1 || { echo "docker does not answer: start it and try again" >&2; exit 1; }
}

ensure_cluster() {
    if kind get clusters 2> /dev/null | grep -qx "$CLUSTER"; then
        echo "==> the kind cluster $CLUSTER is there"
    else
        echo "==> creating the kind cluster $CLUSTER"
        kind create cluster --name "$CLUSTER" --wait 120s
    fi
    echo "==> loading $IMAGE into it"
    kind load docker-image "$IMAGE" --name "$CLUSTER"
}

# $OVERLAY: what deploy/overlays/local has, but with the manifests that the image renders for
# CONFIG (its modules, replicas and resources) instead of the committed ones, and CONFIG, with the
# geko-<module>.toml files beside it, as the config map.
prepare() {
    rm -rf "$OVERLAY"
    mkdir -p "$OVERLAY"
    cp deploy/overlays/local/namespace.yaml deploy/overlays/local/gateway.yaml deploy/overlays/local/gateway.conf "$OVERLAY/"
    cp "$CONFIG" "$OVERLAY/geko.toml"
    local siblings="" file
    for file in "$(dirname "$CONFIG")"/geko-*.toml; do
        [ -f "$file" ] || continue
        cp "$file" "$OVERLAY/"
        siblings="$siblings      - $(basename "$file")\n"
    done
    docker run --rm -e RUST_LOG=off -v "$CONFIG":/etc/geko/geko.toml:ro "$IMAGE" k8s render > "$OVERLAY/resources.yaml"
    sed -e 's|^  - ../../base$|  - resources.yaml|' \
        -e "s|^      - geko.toml\$|      - geko.toml\n${siblings%\\n}|" \
        deploy/overlays/local/kustomization.yaml | grep -v '^$' > "$OVERLAY/kustomization.yaml"
}

# Builds a module with the cron job that renders for it, and waits for the job: they run one after
# the other, as they share the downloads.
build() {
    local module=$1 job ok bad deadline logs
    job="first-$module-$(date +%s)"
    echo "==> building $module (job $job)"
    kubectl -n "$NAMESPACE" create job "$job" --from="cronjob/geko-build-$module" > /dev/null
    kubectl -n "$NAMESPACE" logs -f "job/$job" --pod-running-timeout=300s 2>&1 | sed "s/^/[$module] /" &
    logs=$!
    deadline=$(($(date +%s) + BUILD_WAIT))
    while :; do
        read -r ok bad <<< "$(kubectl -n "$NAMESPACE" get job "$job" -o jsonpath='{.status.succeeded} {.status.failed}')"
        [ "${ok:-0}" -ge 1 ] && break
        if [ "${bad:-0}" -ge 1 ]; then
            wait "$logs" 2> /dev/null || true
            echo "the build of $module failed: kubectl --context $CONTEXT -n $NAMESPACE logs job/$job" >&2
            return 1
        fi
        [ "$(date +%s)" -lt "$deadline" ] || { echo "the build of $module took more than ${BUILD_WAIT}s" >&2; return 1; }
        sleep 5
    done
    wait "$logs" 2> /dev/null || true
}

PIDFILE=$OVERLAY/port-forward.pid

stop_forward() {
    if [ -f "$PIDFILE" ]; then
        kill "$(cat "$PIDFILE")" 2> /dev/null || true
        rm -f "$PIDFILE"
    fi
}

up() {
    local follow=${GEKO_DEV_FOLLOW:-1} module
    [ "${1:-}" != --no-follow ] || follow=0
    check
    ensure_cluster
    prepare
    echo "==> applying the manifests"
    kubectl apply -k "$OVERLAY"
    for module in ${MODULES//,/ }; do
        build "$module"
    done
    for module in ${MODULES//,/ } gateway; do
        # Not `rollout status`: a module pod is not ready until its build has published, which can take
        # longer than the progress deadline of its deployment, and that one would fail for it.
        kubectl -n "$NAMESPACE" wait --for=condition=Available "deployment/geko-$module" --timeout=600s
    done
    stop_forward
    # The servers are ready when they have loaded what the builds published: every 30 s they look.
    nohup "$(type -P kubectl)" --context "$CONTEXT" -n "$NAMESPACE" port-forward "svc/geko-gateway" "$PORT:80" > "$OVERLAY/port-forward.log" 2>&1 &
    echo $! > "$PIDFILE"
    echo "geko-dev ($MODULES), a pod per module, serves http://localhost:$PORT/ (demo) and /docs (API)."
    if [ "$follow" = 1 ]; then
        echo "Its log follows; Ctrl-C leaves it running."
        kubectl -n "$NAMESPACE" logs -f -l 'app.kubernetes.io/part-of=geko,app.kubernetes.io/component!=gateway' --prefix --tail=5 --max-log-requests=10
    fi
}

down() {
    stop_forward
    if [ "${1:-}" = --purge ]; then
        kind delete cluster --name "$CLUSTER"
        rm -rf "$OVERLAY"
        return
    fi
    kind get clusters 2> /dev/null | grep -qx "$CLUSTER" || { echo "there is no cluster $CLUSTER"; return; }
    # Everything but the volume with the bundles and the downloads.
    kubectl -n "$NAMESPACE" delete deployment,service,cronjob,job,ingress,configmap --all --ignore-not-found
}

case ${1:-} in
    check) check ;;
    up) up "${2:-}" ;;
    down) down "${2:-}" ;;
    *) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2 ;;
esac
