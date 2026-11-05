#!/usr/bin/env bash
# land.sh — commit the working copy, preserving the reference/lcms2 gitlink.
#
# WHY: jj working copies do not model git submodules. A `jj describe` commit
# silently omits `reference/lcms2` from the tree (it appears as `D
# reference/lcms2` in `jj st`). Every landing therefore grafts the gitlink
# onto the jj-described tip via `git commit-tree`.
#
# Usage: tools/land.sh "commit message"
set -euo pipefail
cd "$(dirname "$0")/../.."

SUB_SHA=$(git -C reference/lcms2 rev-parse HEAD)
MSG="${1:?commit message required}"

git add -A
git update-index --add --cacheinfo "160000,$SUB_SHA,reference/lcms2"
TREE=$(git write-tree)
PARENT=$(git rev-parse HEAD)
NEW=$(git commit-tree "$TREE" -p "$PARENT" -m "$MSG")
git update-ref refs/heads/main "$NEW"
jj git import
jj git push --bookmark main
git merge-base --is-ancestor "$NEW" origin/main && echo "PUSH VERIFIED $NEW"
git ls-tree origin/main reference/ | grep -q 160000 && echo "GITLINK OK"
