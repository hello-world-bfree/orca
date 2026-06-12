# Project

A self-hosted **internal fork of Windmill**, for personal and internal business use only —
never sold, served to third parties, or shared outside the org. We stand up a faithful
baseline close to upstream, then customize. Full context: `__docs/project_brief.md`.

## Model
- This repo is a direct fork of windmill-labs/windmill, pinned via the `upstream` remote;
  `git diff upstream/<tag>` shows our divergence.
- Build **from source with default features only** — never the `enterprise`/`private`
  features, and never the prebuilt Community Edition image.
- Dev build deltas live in `docker-compose.override.yml` (tracked `docker-compose.yml` stays
  pristine vs upstream); build with `features="oss_core,python"`.

## Remotes & branch topology
Three remotes, one shared base, two divergent leaves (siblings):
- `upstream` (windmill-labs) — read-only source of updates; never pushed to.
- `master` — shared base: upstream + shared deviations. The ONLY branch that merges
  upstream. Mirrored to `origin/base` + `hallow/base`.
- `hallow` → `hallow/master` — base + company/internal workflows.
- `personal` → `origin/master` — base + personal/exploratory work.

Placement: shared deviation → `master`; company-only → `hallow`; personal-only → `personal`.
Fan out after every base change: `git merge master` on each leaf. Promote a personal
experiment company-ward via cherry-pick (`personal`→`hallow`). Leaves never merge
upstream, so `merge=ours` only matters on the upstream→`master` merge.

## Deliberate deviations from upstream (keep each as a named, isolated commit)
- `check_nb_of_user` neutered to `Ok(())` (removes the 10-SSO / 50-account cap).
- CE git-sync cap neutered: `CE_GIT_SYNC_MAX_USERS` removed, `check_git_sync_access` /
  `get_git_sync_enabled` un-gated (git-sync works with >2 members / >1 repo).
- Rebranded: own logo/favicon/name, swapped at the same asset paths and in visible strings;
  internal identifiers left as upstream.
- Enterprise/`private` code is never enabled or compiled.

## Disciplines
- **Keep every customization as small and isolated as possible.** Minimal surface area is the
  whole point — it's what keeps upstream merges simple and clean. The labeled-commit / `merge=ours`
  split exists to serve this, not the reverse.
- Never enable or compile proprietary `enterprise`/`private` features.
- No silent in-place edits to upstream-tracked code — every deviation is a tracked, labeled
  patch, so upstream syncs surface conflicts instead of reverting us.
- Rebrand the surface, not the guts (don't rename crates, the `wmill` CLI, or DB tables).
- Build from source, not the CE image.
- Licensing/trademark rationale lives in `__docs/project_brief.md` — don't re-derive it inline.

## Upstream sync
- Ritual + tooling: `__docs/upstream-sync-runbook.md`; scripts in `__scripts/`
  (`sync-preflight.sh` = go/no-go, `sync-merge.sh` = throwaway-branch merge, `verify-deviations.sh`).
- Merge upstream on a throwaway `sync-<tag>` branch, build + smoke there, then `git merge --ff-only`
  into `master` — never merge upstream straight into `master`.
- **Per clone, NOT committed:** `git config merge.ours.driver true`. If unset, the `.gitattributes`
  `merge=ours` lines silently no-op and upstream overwrites our owned files
  (`/CLAUDE.md`, `frontend/static/logo.svg`).
- Pinned tag of record: `__docs/project_brief.md` §Status — bump on each sync.

## Relationship to the `hallow-windmill` plugin
Two layers describing the **same deployed Windmill** from opposite ends — different jobs, must stay aligned:
- **This repo (`orca`) = source side.** The Windmill fork we build from source (OSS-only) and
  self-host. Editing it changes the binary/instance. This is the "platform repo" the plugin's
  README points to for infra-ops.
- **`hallow-windmill` plugin = operator side.** How Hallow engineers author scripts/flows/triggers
  on the *running* instance (`windmill.platform.hallow.app`, `dev` workspace) via the `wmill` CLI +
  Windmill MCP. It documents that instance's runtime behavior, not our source.

Alignment to hold (the running instance is built from this fork):
- **Version** — the plugin's documented behavior assumes the instance's Windmill version. Keep the
  deployment on this fork's pinned tag (`__docs/project_brief.md` §Status); on each upstream sync confirm
  plugin assumptions still hold.
- **OSS-only feature set** — the plugin treats EE features as absent (Kafka/NATS/MQTT/SQS/GCP/Azure/
  Postgres-CDC/WebSocket triggers, multiplayer), matching our "never compile `enterprise`/`private`"
  discipline. Don't enable an EE feature here without updating the plugin's assumptions.
- **Our deviations leak into the instance** — neutered caps change runtime behavior the plugin sees
  (no 10-SSO/50-account cap; git-sync works with >2 members / >1 repo). Stock-CE assumptions in
  plugin docs would be wrong against this fork.

Don't author user scripts/flows in *this* repo — that's the plugin's job against the running
instance. This repo only builds and customizes the Windmill we deploy.
