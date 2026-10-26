#!/usr/bin/env bash
# The checks of every change of the host, the same locally and on the CI (GitHub Actions): the CI
# files only prepare Rust and the cache, then run this script. The modules check themselves, each in
# its repository (candymaps/geko-geocoder, candymaps/geko-tiles), with the host of their contract.
#
#   ci/test.sh [--variant <variants>]
#
# --variant, what is checked, comma-separated (default: all of them):
#   ui            the dashboard (ui/): typechecks and compiles it (ci/ui.sh, in Docker); it runs first,
#                 so that the checks below compile geko with it embedded
#   code          clippy (warnings are errors), dependency rules (what each crate of the host and
#                 each technology may use), and licenses and security advisories (cargo-deny)
#   contracts     unit tests and the conformance suites of the SDK: the module contract through
#                 its adapter (dylib), and the Downloader
#   deploy        the manifests of deploy/ (ci/deploy.sh check): up to date with what geko k8s render
#                 writes, and valid; without a cluster
set -euo pipefail
cd "$(dirname "$0")/.."

lint() {
    rustup component list --installed 2>/dev/null | grep -q '^clippy' || rustup component add clippy
    cargo clippy --workspace --all-targets --locked -- -D warnings
}

test() {
    cargo test --workspace --locked
}

direct_dependencies() { cargo tree -p "$1" -e normal --depth 1 --prefix none --locked | tail -n +2 | cut -d' ' -f1 | sort -u; }

# Crates of this workspace that depend on `$1` directly.
direct_dependents() {
    cargo tree -i "$1" -e normal --depth 1 --prefix none --workspace --locked 2>/dev/null | tail -n +2 \
        | grep -F " ($PWD/" | cut -d' ' -f1 | sort -u
}

# Fails, naming them, if `found` (one per line) has any name not in `allowed` (space-separated).
only() {
    local what=$1 allowed=$2 found=$3 name bad=""
    for name in $found; do
        [[ " $allowed " == *" $name "* ]] || bad="$bad $name"
    done
    [ -z "$bad" ] || { echo "$what:$bad" >&2; return 1; }
}

# The rules of AGENTS.md that the dependency graph can show: the host knows no module but through
# the contract of the SDK (docs/MODULES_PLAN.md), and each concrete technology stays inside its
# crate. A new user of one of them is either a rule broken or a decision to write down here.
architecture() {
    local failed=0
    only "geko-module-sdk depends on" "hex serde serde_json sha2 thiserror tracing" "$(direct_dependencies geko-module-sdk)" || failed=1
    only "geko-server depends on" "arc-swap axum geko-module-sdk serde serde_json tokio tower-http tracing" "$(direct_dependencies geko-server)" || failed=1
    only "geko-app depends on" "anyhow clap geko-artifact geko-fetch geko-module-sdk geko-server hex libloading mimalloc serde serde_json sha2 tokio toml tracing tracing-subscriber" "$(direct_dependencies geko-app)" || failed=1
    only "ureq is used by" "geko-fetch" "$(direct_dependents ureq)" || failed=1
    only "tokio is used by" "geko-server geko-app" "$(direct_dependents tokio)" || failed=1
    only "axum is used by" "geko-server" "$(direct_dependents axum)" || failed=1
    only "tower-http is used by" "geko-server" "$(direct_dependents tower-http)" || failed=1
    return $failed
}

CARGO_DENY_VERSION=0.20.2

# Licenses of the dependencies (deny.toml) and security advisories of the versions in Cargo.lock.
deny() {
    local bin
    bin=$(command -v cargo-deny || true)
    if [ -z "$bin" ] || ! "$bin" --version | grep -q " $CARGO_DENY_VERSION\$"; then
        local dir="${CARGO_HOME:-$HOME/.cargo}/bin" name="cargo-deny-$CARGO_DENY_VERSION-x86_64-unknown-linux-musl"
        mkdir -p "$dir"
        curl -fsSL "https://github.com/EmbarkStudios/cargo-deny/releases/download/$CARGO_DENY_VERSION/$name.tar.gz" \
            | tar -xzf - -C "$dir" --strip-components=1 "$name/cargo-deny"
        bin="$dir/cargo-deny"
    fi
    "$bin" --locked check
}

ALL_VARIANTS=ui,code,contracts,deploy
VARIANTS=$ALL_VARIANTS

usage() { echo "usage: $0 [--variant $ALL_VARIANTS]" >&2; }

# Whether the variant `$1` was asked for.
wants() { [[ ",$VARIANTS," == *",$1,"* ]]; }

while [ $# -gt 0 ]; do
    case $1 in
        --variant) VARIANTS=${2:?--variant takes a list}; shift 2 ;;
        --variant=*) VARIANTS=${1#*=}; shift ;;
        -h | --help) usage; exit 0 ;;
        *) usage; exit 2 ;;
    esac
done
for variant in ${VARIANTS//,/ }; do
    [[ ",$ALL_VARIANTS," == *",$variant,"* ]] || { echo "unknown variant $variant" >&2; usage; exit 2; }
done

# Runs a check in a subshell, where errexit stops it at its first failure as when it ran alone,
# and goes on with the next one: the summary names the checks that failed.
FAILED=()
run() {
    local name=$1 status
    shift
    echo "==> $name"
    set +e
    (
        set -e
        "$@"
    )
    status=$?
    set -e
    [ "$status" = 0 ] || FAILED+=("$name")
}

wants ui && run ui ci/ui.sh
if wants code; then
    run lint lint
    run architecture architecture
    run deny deny
fi
wants contracts && run test test
wants deploy && run deploy ci/deploy.sh check
if [ ${#FAILED[@]} -gt 0 ]; then
    echo "failed: ${FAILED[*]}" >&2
    exit 1
fi
echo "passed"
