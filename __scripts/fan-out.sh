#!/usr/bin/env bash
# fan-out.sh
#
# Propagates the shared base (master) into both leaves and pushes them.
# Run after master gains a commit — an upstream sync or a shared deviation —
# see the upstream-sync runbook step 6. One-way only: master -> hallow,
# master -> personal. Never the reverse, never leaf <-> leaf.
#
# Stops loud on a dirty tree, a merge conflict, or a rejected push, and pushes
# a leaf only if its merge was clean. Exit 0 = both leaves merged + pushed.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

base=master
leaves=(hallow personal)

# Refuse on uncommitted changes — switching branches into a merge is how you
# leave a leaf half-merged and lose track of which tree you're in.
if ! git diff-index --quiet HEAD --; then
  echo "FAIL  working tree dirty — commit or stash before fanning out." >&2
  exit 1
fi

start=$(git symbolic-ref --short HEAD)

for leaf in "${leaves[@]}"; do
  echo "── $base → $leaf ─────────────────────────────────────────"
  git switch -q "$leaf"
  if ! git merge --no-edit "$base"; then
    echo "FAIL  merge conflict on '$leaf'. Resolve, commit, 'git push', then" >&2
    echo "      re-run (it'll see '$leaf' as already up to date and skip it)." >&2
    exit 1
  fi
  if ! git push; then
    echo "FAIL  push of '$leaf' rejected (remote moved?). Reconcile, then re-run." >&2
    exit 1
  fi
done

git switch -q "$start"
echo "─────────────────────────────────────────────────────────"
echo "fanned out: ${leaves[*]} now carry $base and are pushed."
