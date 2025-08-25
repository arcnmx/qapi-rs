#!/bin/sh
set -e

if [ $# -ne 1 ]; then
    echo "Usage: $0 <tag>"
    exit 1
fi

TAG="$1"

git switch --create="$TAG-filtered" "$TAG"
git filter-repo \
    --path-regex '^qapi-schema\.json$|^qapi/[^/]*\.json$|^qga/[^/]*\.json$|^VERSION$' \
    --prune-empty always \
		--force # --force flag tells `git filter-repo` to proceed even though it doesn't consider this a "fresh clone"
