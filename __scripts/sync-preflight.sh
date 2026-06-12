#!/usr/bin/env bash
# sync-preflight.sh <upstream-tag>
#
# READ-ONLY pre-merge analysis for an upstream sync. Mutates nothing.
# Answers: "will moving to <tag> break any of our deviations?" — before you merge.
#
# Verdict:  GO (0) | REVIEW (1) | NO-GO (2)
#   NO-GO   merge=ours mechanics broken, or a surgical file vanished upstream
#   REVIEW  upstream changed a file we surgically edited, or a cap signal grew
#   GO      our edits untouched upstream, merge=ours intact, no new cap signals
#
# Deviation set is DERIVED, not hardcoded:
#   - surgical (Class-2) = files touched by `deviation:`-labelled commits that
#     still exist upstream at <tag> and are not merge=ours
#   - merge=ours (Class-1) = paths from .gitattributes
set -euo pipefail

NEW="${1:-}"
if [ -z "$NEW" ]; then
  echo "usage: $(basename "$0") <upstream-tag>   e.g. v1.720.0" >&2
  exit 2
fi

cd "$(git rev-parse --show-toplevel)"

if ! git rev-parse -q --verify "refs/tags/$NEW^{commit}" >/dev/null; then
  echo "tag '$NEW' not found locally — run: git fetch upstream --tags" >&2
  exit 2
fi

BASE="$(git merge-base HEAD "refs/tags/$NEW")"   # fork point (= current pinned tag)
FROM="$(git describe --tags --exact-match "$BASE" 2>/dev/null || git rev-parse --short "$BASE")"
STATUS=0
bump() { [ "$1" -gt "$STATUS" ] && STATUS="$1"; return 0; }

echo "── upstream sync preflight ──────────────────────────────"
echo "from $FROM  →  $NEW"
echo

# ── merge=ours (Class-1) files from .gitattributes ───────────
mapfile -t OURS < <(awk '/merge=ours/{sub(/^\//,"",$1); print $1}' .gitattributes 2>/dev/null || true)
in_ours() { local x; for x in "${OURS[@]:-}"; do [ "$x" = "$1" ] && return 0; done; return 1; }

# ── candidate deviation files from labelled commits ──────────
mapfile -t CAND < <(git log --grep='^deviation:' --pretty=format: --name-only "$BASE"..HEAD 2>/dev/null \
                    | sed '/^$/d' | sort -u || true)
if [ "${#CAND[@]}" -eq 0 ]; then
  CAND=(backend/windmill-api/src/oauth2_oss.rs backend/windmill-api-workspaces/src/workspaces.rs)
  echo "note: no deviation: commits in range — using default surgical list"
  echo
fi

# ── 1. classify + blast radius ───────────────────────────────
echo "1. BLAST RADIUS — did upstream touch our files between $FROM and $NEW?"
SURGICAL=()
for f in "${CAND[@]}"; do
  exists_new=0;  git cat-file -e "$NEW:$f"  2>/dev/null && exists_new=1
  exists_base=0; git cat-file -e "$BASE:$f" 2>/dev/null && exists_base=1
  if in_ours "$f"; then
    continue                                   # handled in section 3
  elif [ "$exists_new" = 1 ]; then
    SURGICAL+=("$f")
    n=$(git rev-list --count "$BASE".."$NEW" -- "$f")
    if [ "$n" -eq 0 ]; then
      printf '   ok      %s  (untouched upstream)\n' "$f"
    else
      printf '   REVIEW  %s  (%s upstream commit(s) — re-apply may be needed)\n' "$f" "$n"
      bump 1
    fi
  elif [ "$exists_base" = 1 ]; then
    printf '   NO-GO   %s  (existed at fork, GONE at %s — upstream renamed/deleted)\n' "$f" "$NEW"
    bump 2
  fi   # else: ours-only new file, no upstream conflict surface — skip silently
done
[ "${#SURGICAL[@]}" -eq 0 ] && echo "   (no surgical upstream-tracked files in deviation set)"
echo

# ── 2. semantic drift — new cap enforcement at $NEW ──────────
echo "2. CAP DRIFT — occurrence delta of cap sentinels (BASE → $NEW), backend/ only."
echo "   A clean merge does NOT catch upstream ADDING a fresh cap. Growth = inspect."
patterns=(
  'check_nb_of_user'
  'CE_GIT_SYNC_MAX_USERS'
  'maximum number of'
  'without an enterprise license'
)
for p in "${patterns[@]}"; do
  b=$(git grep -I -c -e "$p" "$BASE" -- backend 2>/dev/null | awk -F: '{s+=$NF} END{print s+0}')
  c=$(git grep -I -c -e "$p" "$NEW"  -- backend 2>/dev/null | awk -F: '{s+=$NF} END{print s+0}')
  if [ "$c" -gt "$b" ]; then
    printf '   REVIEW  %-32s %s → %s  (+%s)\n' "$p" "$b" "$c" "$((c-b))"
    bump 1
  else
    printf '   ok      %-32s %s → %s\n' "$p" "$b" "$c"
  fi
done
echo

# ── 3. merge=ours integrity ──────────────────────────────────
echo "3. merge=ours — Class-1 files must auto-keep OUR version."
if [ "$(git config merge.ours.driver || true)" = "true" ]; then
  echo "   ok      merge.ours.driver = true"
else
  echo "   NO-GO   merge.ours.driver UNSET → .gitattributes silently no-ops, upstream wins"
  echo "           fix: git config merge.ours.driver true"
  bump 2
fi
if [ "${#OURS[@]}" -eq 0 ]; then
  echo "   note    no merge=ours entries in .gitattributes"
fi
for f in "${OURS[@]:-}"; do
  [ -z "$f" ] && continue
  attr=$(git check-attr merge -- "$f" | sed 's/.*: //')
  if [ "$attr" = "ours" ]; then
    printf '   ok      %s  → merge: ours\n' "$f"
  else
    printf '   NO-GO   %s  → merge: %s  (expected ours)\n' "$f" "$attr"
    bump 2
  fi
done
echo

# ── 4. predicted merge conflicts (read-only merge-tree) ──────
echo "4. PREDICTED CONFLICTS — git merge-tree (no worktree change)."
if out=$(git merge-tree --write-tree --name-only HEAD "refs/tags/$NEW" 2>/dev/null); then
  echo "   ok      merge is conflict-free"
else
  echo "   REVIEW  conflicts predicted in:"
  echo "$out" | tail -n +2 | sed 's/^/             /'
  bump 1
fi
echo

# ── verdict ──────────────────────────────────────────────────
echo "─────────────────────────────────────────────────────────"
case "$STATUS" in
  0) echo "VERDICT: GO — proceed with the throwaway-branch merge below." ;;
  1) echo "VERDICT: REVIEW — safe to try, but read the flagged items first." ;;
  2) echo "VERDICT: NO-GO — fix the NO-GO items before merging." ;;
esac
cat <<EOF

Next — merge on a throwaway branch so master never ends up half-broken:
  git switch -c sync-$NEW
  git merge $NEW                 # resolve conflicts, re-apply Ok(()) bodies
  __scripts/sync-merge.sh $NEW   # (or run this to automate the above + checks)
  # build + smoke 1 Python + 1 Bun job, THEN fold:
  git switch master && git merge --ff-only sync-$NEW && git branch -d sync-$NEW
  # finally bump the pinned tag in __docs/project_brief.md §Status to $NEW
EOF
exit "$STATUS"
