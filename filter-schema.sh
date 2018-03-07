#!/bin/sh
set -e

cd ./schema/
git filter-branch \
	--prune-empty \
	--tag-name-filter cat \
	--index-filter '
		git ls-tree -z -r --name-only --full-tree $GIT_COMMIT \
		| grep -z -v "^qapi-schema\.json$" \
		| grep -z -v "^qapi/.*\.json$" \
		| grep -z -v "^qga/.*\.json$" \
		| grep -z -v "^VERSION$" \
		| xargs -0 -r git rm --cached -r
	' \
	-- \
	--all
