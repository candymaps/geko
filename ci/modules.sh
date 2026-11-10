#!/usr/bin/env bash
# The exact modules of this geko (docs/MODULE_UPDATES_PLAN.md): those of modules.lock, which each
# module publishes from its repository in $GEKO_MODULES_REGISTRY/geko-<module> (ghcr.io/candymaps by
# default), read without a login, since they are public and a stale login would only deny them:
#
#   ci/modules.sh pull <folder>           downloads each package by its digest into <folder>/<module>/
#                                         and checks its signature, its name, version and platform
#   ci/modules.sh conformance <folder>    loads each package of <folder> and runs the conformance
#                                         suite of the SDK on it
#   ci/modules.sh smoke <folder>          builds and serves Luxembourg with the packages of <folder>,
#                                         in $GEKO_SMOKE_DATA (/var/lib/geko/modules-smoke by
#                                         default, so that a second run reuses its downloads), and
#                                         asks each module for an answer
#   ci/modules.sh verify [folder]         the three, in <folder> ($CARGO_TARGET_DIR/modules by default)
#   ci/modules.sh update                  raises modules.lock to the newest release of each module
set -euo pipefail
cd "$(dirname "$0")/.."

registry=${GEKO_MODULES_REGISTRY:-ghcr.io/candymaps}
target=${CARGO_TARGET_DIR:-target}
lock=modules.lock
port=${GEKO_SMOKE_PORT:-18080}
server=

fail() { echo "FAILED: $*" >&2; exit 1; }

# The lines of modules.lock: <module> <version> <platform> <digest>.
locked() { tr -d '\r' < "$lock" | grep -v -e '^#' -e '^$'; }

field() { tr -d '\r' < "$2" | sed -n "s/^$1 *= *\"\{0,1\}\([^\"]*\)\"\{0,1\} *\$/\1/p" | head -1; }

# `oras` without the credentials of this machine.
anonymous() {
    local config status=0
    config=$(mktemp)
    echo '{}' > "$config"
    oras "$@" --registry-config "$config" < /dev/null || status=$?
    rm -f "$config"
    return $status
}

pull() {
    local out=${1:?usage: ci/modules.sh pull <folder>} name version platform digest dir
    cargo build --locked -p geko-artifact --bin geko-module-sign
    while read -r name version platform digest; do
        dir=$out/$name
        rm -rf "${dir:?}"
        mkdir -p "$dir"
        anonymous pull --output "$dir" "$registry/geko-$name@$digest" > /dev/null
        "$target/debug/geko-module-sign" verify "$dir"
        [ "$(field name "$dir/module.toml")" = "$name" ] || fail "$dir is not the module $name"
        [ "$(field version "$dir/module.toml")" = "$version" ] || fail "$dir is not version $version"
        [ "$(field platform "$dir/module.toml")" = "$platform" ] || fail "$dir is not for $platform"
        echo "pulled $name $version ($digest)"
    done < <(locked)
}

conformance() {
    local out=${1:?usage: ci/modules.sh conformance <folder>}
    GEKO_CHECK_PACKAGES=$(cd "$out" && pwd) cargo test --locked -p geko-app --lib real_packages -- --ignored --nocapture
}

# The answer to `path` has `text` in it.
answers() {
    local path=$1 text=$2
    curl -fsS --max-time 30 "http://127.0.0.1:$port$path" | grep -q "$text" || fail "$path does not answer with $text"
    echo "ok   $path"
}

stop_server() {
    [ -z "$server" ] || kill "$server" 2> /dev/null || true
}

smoke() {
    local out=${1:?usage: ci/modules.sh smoke <folder>} data=${GEKO_SMOKE_DATA:-/var/lib/geko/modules-smoke} geko
    mkdir -p "$data"
    cargo build --profile ci --locked -p geko-app --bin geko
    geko=$target/ci/geko
    GEKO_MODULES=$(cd "$out" && pwd)
    export GEKO_MODULES GEKO_DATA=$data GEKO_CONFIG=$PWD/ci/modules-smoke.toml
    # This checks the modules, not the sources: once a run has every download, the next ones do not
    # ask for them, or a source that is down would fail them. Only if that fails, as with a module
    # that needs a new source, does it go to the network. Every stage runs again (--force): a stage
    # skipped would check nothing of the module, and its key does not always know the code changed.
    if [ -f "$data/.downloaded" ]; then
        sed 's/^\[data\]$/&\noffline = true/' "$GEKO_CONFIG" > "$data/offline.toml"
        GEKO_CONFIG=$data/offline.toml "$geko" build --force || "$geko" build --force
    else
        "$geko" build --force
    fi
    touch "$data/.downloaded"
    trap stop_server EXIT
    "$geko" serve --listen "127.0.0.1:$port" &
    server=$!
    for _ in $(seq 1 120); do
        [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "http://127.0.0.1:$port/ready" || true)" != 200 ] || break
        kill -0 "$server" 2> /dev/null || fail "geko serve stopped"
        sleep 1
    done
    answers /ready '"ready":true'
    answers '/geocoder/v1/search?text=Luxembourg' '"features"'
    answers /tiles/tilejson.json '"tiles"'
    answers '/router/v1/route?from=49.6116,6.1319&to=49.4958,5.9806' '"duration'
}

verify() {
    local out=${1:-$target/modules}
    pull "$out"
    conformance "$out"
    smoke "$out"
}

# Pre-releases and the versions of a commit (<version>-<commit>-<platform>) do not count: only a
# release reaches a geko.
update() {
    local name version platform digest newest
    while read -r name version platform digest; do
        newest=$(anonymous repo tags "$registry/geko-$name" | tr -d '\r' \
            | sed -n "s/^\([0-9]*\.[0-9]*\.[0-9]*\)-$platform\$/\1/p" | sort -V | tail -1)
        [ -n "$newest" ] || fail "no release of $name for $platform"
        if [ "$newest" = "$version" ]; then
            echo "$name $version is the newest"
            continue
        fi
        digest=$(anonymous resolve "$registry/geko-$name:$newest-$platform")
        sed -i "s|^$name .*|$name $newest $platform $digest|" "$lock"
        echo "$name $version -> $newest"
    done < <(locked)
}

case "${1:-}" in
    pull) shift; pull "$@" ;;
    conformance) shift; conformance "$@" ;;
    smoke) shift; smoke "$@" ;;
    verify) shift; verify "$@" ;;
    update) update ;;
    *) echo "usage: ci/modules.sh pull <folder> | conformance <folder> | smoke <folder> | verify [folder] | update" >&2; exit 2 ;;
esac
