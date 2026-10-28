#!/usr/bin/env bash
# The checks of ci/test.sh that what changed since a commit can fail, for the CI of pull requests
# (whether to run them) and `make test-changed`:
#
#   ci/affected.sh <base> [<head>]
#
# prints the arguments for ci/test.sh (`--variant code,contracts,deploy`, or `--variant deploy` when
# only the manifests changed) for what changed from <base> to <head> (without it, to the working
# tree), or nothing when no check covers it (only documentation and what the checks do not build:
# the image, the development setup, the CI).
set -euo pipefail
cd "$(dirname "$0")/.."

base=${1:?usage: ci/affected.sh <base> [<head>]}
range=("$base" ${2:+"$2"})

deploy=0
while IFS= read -r file; do
    case $file in
        *.md | docs/* | jekyll/* | LICENSE | develop/* | Dockerfile | .dockerignore | .github/* | Makefile | make.cmd | ci/affected.sh) ;;
        deploy/* | ci/deploy.sh) deploy=1 ;;
        *) echo "--variant code,contracts,deploy"; exit 0 ;;
    esac
done < <(git diff --name-only "${range[@]}")
[ "$deploy" = 0 ] || echo "--variant deploy"
