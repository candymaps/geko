#!/usr/bin/env bash
# The checks of ci/test.sh that what changed since a commit can fail, for `.githooks/pre-push`
# and `make test-changed`:
#
#   ci/affected.sh <base> [<head>]
#
# prints the arguments for ci/test.sh (`--variant code,contracts`) for what changed from <base> to
# <head> (without it, to the working tree), or nothing when no check covers it (only
# documentation and what the checks do not build: the image, the development setup, the CI).
set -euo pipefail
cd "$(dirname "$0")/.."

base=${1:?usage: ci/affected.sh <base> [<head>]}
range=("$base" ${2:+"$2"})

while IFS= read -r file; do
    case $file in
        *.md | docs/* | LICENSE | CLA.md | develop/* | Dockerfile | .dockerignore | .github/* | .githooks/* | Makefile | make.cmd | ci/affected.sh) ;;
        *) echo "--variant code,contracts"; exit 0 ;;
    esac
done < <(git diff --name-only "${range[@]}")
