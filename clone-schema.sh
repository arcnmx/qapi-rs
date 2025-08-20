#!/usr/bin/env bash
set -eu

if [ $# -ne 1 ]; then
    echo "Usage: $0 <tag>"
    exit 1
fi

TAG="$1"
OLD_PWD=$PWD

git clone https://github.com/qemu/qemu.git /tmp/qemu
cd /tmp/qemu

if ! git rev-parse --verify "refs/tags/$TAG" >/dev/null 2>&1; then
    echo "Error: Tag '$TAG' does not exist in the repository"
    exit 1
fi

$OLD_PWD/filter-schema.sh "$TAG"
git remote add filtered git@github.com:arcnmx/qemu-qapi-filtered.git
git push filtered --tags --quiet
# the branch "$TAG-filtered" is created by filter-schema.sh
git push filtered "$TAG-filtered"