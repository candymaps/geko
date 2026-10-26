#!/usr/bin/env bash
# The dashboard (ui/): typechecks it and compiles it into ui/dist, which geko-server embeds in the
# binary (crates/geko-server/build.rs):
#
#   ci/ui.sh
#
# It runs in Docker (node:22-slim, the Node of the packages of ui/package-lock.json), so that no
# Node is needed on the machine, and works on a copy of ui/: the packages of the host (those of
# another system, if it is not Linux) are neither read nor replaced.
set -euo pipefail
cd "$(dirname "$0")/.."

rm -rf ui/dist
mkdir -p ui/dist
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp \
    -v "$PWD/ui:/src:ro" -v "$PWD/ui/dist:/out" -w /tmp node:22-slim sh -c '
        set -e
        mkdir work
        tar -C /src --exclude=node_modules --exclude=dist -cf - . | tar -C work -xf -
        cd work
        npm ci --no-audit --no-fund
        npm run build
        cp -r dist/. /out/'
