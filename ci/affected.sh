#!/usr/bin/env bash
# The checks of ci/test.sh that what changed since a commit can fail, for the CI of pull requests
# (whether to run them) and `make test-changed`:
#
#   ci/affected.sh <base> [<head>]
#
# prints the arguments for ci/test.sh (`--variant code,contracts`) for what changed from <base> to
# <head> (without it, to the working tree), or nothing when no check covers it (only
# documentation and what the checks do not build: the image, the development setup, the CI). A
# change that only touches the dashboard (ui/) cannot fail the checks of the Rust code: it is
# embedded in the binary, but compiled apart (ci/ui.sh), and the Rust code builds without it.
set -euo pipefail
cd "$(dirname "$0")/.."

base=${1:?usage: ci/affected.sh <base> [<head>]}
range=("$base" ${2:+"$2"})

ui=0
rust=0
while IFS= read -r file; do
    case $file in
        *.md | docs/* | LICENSE | develop/* | Dockerfile | .dockerignore | .github/* | Makefile | make.cmd | ci/affected.sh) ;;
        ui/* | ci/ui.sh) ui=1 ;;
        *) rust=1 ;;
    esac
done < <(git diff --name-only "${range[@]}")

variants=()
[ "$ui" = 1 ] && variants+=(ui)
[ "$rust" = 1 ] && variants+=(code contracts)
[ ${#variants[@]} -eq 0 ] || echo "--variant $(IFS=,; echo "${variants[*]}")"
exit 0
