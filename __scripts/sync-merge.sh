#!/usr/bin/env bash
# sync-merge.sh <upstream-tag>
#
# Performs an upstream sync on a THROWAWAY branch (sync-<tag>) so a messy merge
# never leaves master half-broken. Runs preflight first, merges on the branch,
# then verifies our deviations survived. Never touches master — you fold it in
# manually (--ff-only) only after build + smoke pass.
set -euo pipefail

NEW="${1:-}"
if [ -z "$NEW" ]; then
  echo "usage: $(basename "$0") <upstream-tag>   e.g. v1.720.0" >&2
  exit 2
fi

cd "$(git rev-parse --show-toplevel)"
HERE="$(cd "$(dirname "$0")" && pwd)"

# ── preflight gate: NO-GO (2) blocks; REVIEW (1) proceeds (branch is safe) ──
set +e
"$HERE/sync-preflight.sh" "$NEW"; pf=$?
set -e
if [ "$pf" -eq 2 ]; then
  echo; echo "preflight = NO-GO — fix blocking items before merging. Aborting."
  exit 1
fi

# ── clean tree required ──
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "working tree not clean — commit/stash first." >&2
  exit 1
fi

BR="sync-$NEW"
if git rev-parse -q --verify "refs/heads/$BR" >/dev/null; then
  echo "branch $BR already exists — delete or rename it first: git branch -D $BR" >&2
  exit 1
fi

START="$(git rev-parse --abbrev-ref HEAD)"
echo; echo "── merging $NEW on throwaway branch $BR (from $START) ──"
git switch -c "$BR"

if ! git merge --no-edit "$NEW"; then
  echo
  echo "CONFLICTS — resolve them, re-apply the Ok(()) neuter bodies, then:"
  echo "   git commit"
  echo "   $HERE/verify-deviations.sh        # confirm caps still neutered"
  echo "   docker compose build && smoke 1 Python + 1 Bun job"
  echo "   git switch $START && git merge --ff-only $BR && git branch -d $BR"
  exit 0
fi

echo; echo "clean merge. verifying deviations survived…"
"$HERE/verify-deviations.sh" || { echo "deviation check FAILED — inspect before folding."; exit 1; }

cat <<EOF

clean merge + deviations intact on $BR. master is untouched.
NOW (still on $BR):
  docker compose build
  docker compose up -d --pull never --force-recreate \\
      windmill_server windmill_worker windmill_worker_native
  # smoke: run 1 Python + 1 Bun job from the UI (the baseline gate)

THEN fold into $START (fast-forward only — fails if anything diverged):
  git switch $START && git merge --ff-only $BR && git branch -d $BR

FINALLY bump the pinned tag in __docs/project_brief.md §Status to $NEW.
EOF
