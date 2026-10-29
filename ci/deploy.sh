#!/usr/bin/env bash
# The manifests of the operator mode in deploy/ (docs/OPERATOR_MODE_PLAN.md), without a cluster.
# They are not edited: `geko k8s render` writes deploy/base/resources.yaml from deploy/geko.toml (what
# the operator makes, for a cluster without it) and `geko k8s install` writes
# deploy/operator/resources.yaml (the operator itself).
#
#   ci/deploy.sh render   writes both again
#   ci/deploy.sh check    fails if one is not what geko writes now (a change of the code or of
#                         deploy/geko.toml without rendering), if an overlay does not build with
#                         kustomize, if what it builds does not pass kubeconform, or if the wizard of
#                         the docs (jekyll/assets/js/geko-wizard.js) writes another operator.yaml
set -euo pipefail
cd "$(dirname "$0")/.."

KUBECONFORM_VERSION=0.6.7
# From CHECKSUMS of that release, for kubeconform-linux-amd64.tar.gz.
KUBECONFORM_SHA256=95f14e87aa28c09d5941f11bd024c1d02fdc0303ccaa23f61cef67bc92619d73
KUBERNETES_VERSION=1.31.0
RESOURCES=deploy/base/resources.yaml
INSTALL=deploy/operator/resources.yaml

# What `geko k8s $1` writes for deploy/geko.toml, in the file `$2`. Without logs: the manifests go
# to the standard output.
render_to() {
    local command=$1 out=$2
    shift 2
    RUST_LOG=off GEKO_CONFIG=deploy/geko.toml cargo run --locked -q -p geko-app --bin geko -- k8s "$command" "$@" > "$out"
}

conform() {
    local bin
    bin=$(command -v kubeconform || true)
    if [ -z "$bin" ] || ! "$bin" -v | grep -q "v\?$KUBECONFORM_VERSION\$"; then
        local dir="${CARGO_HOME:-$HOME/.cargo}/bin" tarball
        mkdir -p "$dir"
        tarball=$(mktemp)
        curl -fsSL "https://github.com/yannh/kubeconform/releases/download/v$KUBECONFORM_VERSION/kubeconform-linux-amd64.tar.gz" -o "$tarball"
        # What was downloaded is the release it names, or nothing is installed.
        echo "$KUBECONFORM_SHA256  $tarball" | sha256sum -c - > /dev/null || { rm -f "$tarball"; echo "kubeconform $KUBECONFORM_VERSION: the download does not match its sha256" >&2; return 1; }
        tar -xzf "$tarball" -C "$dir" kubeconform
        rm -f "$tarball"
        bin="$dir/kubeconform"
    fi
    "$bin" "$@"
}

# What an overlay builds: kustomize, or the one in kubectl.
overlay_manifests() {
    if command -v kustomize > /dev/null; then kustomize build "$1"; else kubectl kustomize "$1"; fi
}

# The wizard of the docs writes the operator.yaml of `geko k8s install`, with the defaults and with
# every option set. `$1` is a file to write geko's output to.
wizard() {
    if ! command -v node > /dev/null; then
        [ -z "${CI:-}" ] || { echo "the wizard check needs node" >&2; return 1; }
        echo "node is not installed: the wizard of the docs is not checked" >&2
        return 0
    fi
    local options=(--tag 0.2.0 --storage 50Gi --storage-class nfs-client --access-mode ReadWriteOnce --host geko.example.org
        --image-prefix registry.example.org/geko/geko- --operator-image registry.example.org/geko/geko:0.2.0)
    node ci/wizard-check.js "$INSTALL" || return 1
    render_to install "$1" "${options[@]}"
    node ci/wizard-check.js "$1" "${options[@]}"
}

check() {
    local status=0 fresh
    fresh=$(mktemp)
    trap "rm -f $fresh" EXIT
    local command file
    for command in render install; do
        file=$RESOURCES
        [ "$command" = render ] || file=$INSTALL
        render_to "$command" "$fresh"
        if ! diff -u "$file" "$fresh"; then
            echo "$file is not what \`geko k8s $command\` writes: run ci/deploy.sh render and commit it" >&2
            status=1
        fi
    done
    wizard "$fresh" || status=1
    local overlay
    for overlay in deploy/overlays/*/; do
        echo "==> $overlay"
        overlay_manifests "$overlay" | conform -strict -summary -kubernetes-version "$KUBERNETES_VERSION" || status=1
    done
    return $status
}

case ${1:-} in
    render) render_to render "$RESOURCES"; echo "wrote $RESOURCES"; render_to install "$INSTALL"; echo "wrote $INSTALL" ;;
    check) check ;;
    *) echo "usage: $0 render|check" >&2; exit 2 ;;
esac
