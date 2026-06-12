#!/usr/bin/env bash
# verify-deviations.sh
#
# Asserts our deviations are still in force in the CURRENT working tree.
# Run after any upstream merge: if a merge silently re-introduced a CE cap,
# this fails loud. Exit 0 = all deviations intact, 1 = at least one reverted.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

fail=0
ok()  { printf '   ok    %s\n' "$1"; }
bad() { printf '   FAIL  %s\n' "$1"; fail=1; }

echo "── deviation integrity ──────────────────────────────────"

# Cap A — account cap must be neutered: no cap messages anywhere in backend.
if git grep -qI -e 'maximum number of oauth users accounts' \
                -e 'maximum number of accounts (50)' -- backend; then
  bad "Cap A: account-cap message present — check_nb_of_user un-neutered"
else
  ok "Cap A: account cap neutered (no cap messages)"
fi
# function must still exist (guard against false-pass via file deletion)
if git grep -qI 'fn check_nb_of_user' -- backend; then
  ok "Cap A: check_nb_of_user present"
else
  bad "Cap A: check_nb_of_user missing — upstream moved/renamed it"
fi

# Cap B — git-sync cap must be gone: no const, no limit message.
if git grep -qI 'CE_GIT_SYNC_MAX_USERS' -- backend; then
  bad "Cap B: CE_GIT_SYNC_MAX_USERS reintroduced"
else
  ok "Cap B: CE_GIT_SYNC_MAX_USERS absent"
fi
if git grep -qI 'Git sync is available for workspaces with up to' -- backend; then
  bad "Cap B: git-sync limit message present — check_git_sync_access un-neutered"
else
  ok "Cap B: git-sync access check neutered"
fi

echo "─────────────────────────────────────────────────────────"
if [ "$fail" -eq 0 ]; then
  echo "all deviations intact."
else
  echo "DEVIATION REVERTED — re-apply the Ok(()) neuter(s) before folding to master."
fi
exit "$fail"
