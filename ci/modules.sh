#!/usr/bin/env bash
# Whether the modules of ci/modules are published for the contract of this geko, which loads no
# module of another contract (docs/MODULES_PLAN.md). Each module publishes itself, from its
# repository, in $GEKO_MODULES_REGISTRY/geko-<module> (ghcr.io/candymaps by default), with the tag
# contract-<n>-<platform>, the newest of its contract:
#
#   ci/modules.sh check    fails, naming them, unless every module of ci/modules is published for
#                          the contract of crates/geko-module-sdk on this platform (linux-x86_64 on
#                          the CI, that of the image)
#
# Needs `oras` and a login to the registry with read access to the packages of the modules.
set -euo pipefail
cd "$(dirname "$0")/.."

registry=${GEKO_MODULES_REGISTRY:-ghcr.io/candymaps}

check() {
    local contract platform name ref missing=0
    contract=$(sed -n 's/^pub const CONTRACT: u32 = \([0-9]*\);/\1/p' crates/geko-module-sdk/src/module.rs)
    # As geko names it (std::env::consts::OS and ARCH), which uname gives on Linux.
    platform="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"
    for name in $(tr -d '\r' < ci/modules | grep -v '^#'); do
        ref="$registry/geko-$name:contract-$contract-$platform"
        if oras manifest fetch "$ref" > /dev/null 2>&1; then
            echo "published: $ref"
        else
            echo "not published: $ref" >&2
            missing=1
        fi
    done
    return $missing
}

case "${1:-}" in
    check) check ;;
    *) echo "usage: ci/modules.sh check" >&2; exit 2 ;;
esac
