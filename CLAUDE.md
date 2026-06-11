# Project

A self-hosted **internal fork of Windmill**, for personal and internal business use only —
never sold, served to third parties, or shared outside the org. We stand up a faithful
baseline close to upstream, then customize. Full context: `__docs/project-brief.md`.

## Model
- This repo is a direct fork of windmill-labs/windmill, pinned via the `upstream` remote;
  `git diff upstream/<tag>` shows our divergence.
- Build **from source with default features only** — never the `enterprise`/`private`
  features, and never the prebuilt Community Edition image.

## Deliberate deviations from upstream (keep each as a named, isolated commit)
- `check_nb_of_user` neutered to `Ok(())` (removes the 10-SSO / 50-account cap).
- Rebranded: own logo/favicon/name, swapped at the same asset paths and in visible strings;
  internal identifiers left as upstream.
- Enterprise/`private` code is never enabled or compiled.

## Disciplines
- Never enable or compile proprietary `enterprise`/`private` features.
- No silent in-place edits to upstream-tracked code — every deviation is a tracked, labeled
  patch, so upstream syncs surface conflicts instead of reverting us.
- Rebrand the surface, not the guts (don't rename crates, the `wmill` CLI, or DB tables).
- Build from source, not the CE image.
- Licensing/trademark rationale lives in `__docs/project-brief.md` — don't re-derive it inline.
