#!/bin/bash
set -eu

OLD_PWD=$PWD

git clone https://github.com/qemu/qemu.git /tmp/qemu
cd /tmp/qemu
$OLD_PWD/filter-schema.sh > /dev/null
git remote add filtered git@github.com:arcnmx/qemu-qapi-filtered.git
git push filtered --tags
git checkout master
git push filtered master
