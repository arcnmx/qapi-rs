#!/usr/bin/env bash
set -eu

REPO_SCHEMA=./schema
if [[ ! -e $REPO_SCHEMA/.git ]]; then
	git submodule update --init $REPO_SCHEMA
fi

if [[ $# -eq 0 ]]; then
	REPO_QEMU="$(realpath "./qemu")"
else
	REPO_QEMU="$(realpath "$1")"
	shift
fi

if [[ ! -e $REPO_QEMU/.git ]]; then
	echo "cloning into $REPO_QEMU ..." >&2
	echo "press enter to continue, or ^C to bail" >&2
	read
	git clone https://gitlab.com/qemu-project/qemu.git "$REPO_QEMU"
fi

# ensure replace refs are in place, since they aren't fetched automatically
if [[ ! -e "$(git -C "$REPO_SCHEMA" rev-parse --git-dir)/refs/replace/" ]]; then
	git -C "$REPO_SCHEMA" fetch origin 'refs/replace/*:refs/replace/*'
fi

git -C "$REPO_SCHEMA" filter-repo \
	--source "$REPO_QEMU" \
	--partial \
	--prune-empty always \
	--replace-refs update-or-add \
	--refs 'refs/tags/*' \
	--tag-rename ":" \
	--force \
	--path-glob 'qapi-schema/*.json' \
	--path-glob 'qapi/*.json' \
	--path-glob 'qga/*.json' \
	--path VERSION
git -C "$REPO_SCHEMA" reflog expire --expire=now --all
git -C "$REPO_SCHEMA" gc --prune=now

LATEST_TAG=$(git -C $REPO_QEMU describe --tags $(git -C $REPO_QEMU rev-list --tags --max-count=1))

# push updated replace refs
echo 'done! now to push the updated replace refs, use:' >&2
echo "git -C $REPO_SCHEMA push origin 'refs/replace/*'" >&2
echo "git -C $REPO_SCHEMA checkout $LATEST_TAG" >&2
