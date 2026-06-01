# 6. CI and developer feedback speed

Date: 2026-05-27

## Status

Accepted

## Context

CI was running the full pipeline (frontend + rust + multi-OS Tauri
build) on every PR including docs-only edits, and Rust compilations
re-built from cold every job because only `target/` was cached. Local
`pre-push` ran the full vitest suite — fast today but a clear cost
ceiling once we add real test files. Repeated `pnpm typecheck` also
re-checked every file even when only one had changed.

## Decision

- **`paths-ignore`** on `ci.yml`, `tauri-build.yml`, `test-required.yml`,
  and `size-limit.yml` for `**/*.md`, `docs/**`, `LICENSE`,
  `.gitignore`, `.editorconfig`. Doc-only PRs skip CI entirely.
  `pr-rules.yml` is intentionally NOT filtered — it validates PR
  metadata that should run on every PR.
- **sccache** (`mozilla-actions/sccache-action@v0.0.6`) added to the
  `rust` job in `ci.yml` and the `build` job in `tauri-build.yml`,
  with `SCCACHE_GHA_ENABLED=true` and `RUSTC_WRAPPER=sccache`. Works
  alongside `Swatinem/rust-cache` — `rust-cache` restores `target/`
  between runs, `sccache` caches individual compile units so
  incremental Rust edits across PRs benefit even when `target/` misses.
- **`tsconfig.json`** gains `incremental: true` and
  `tsBuildInfoFile: ./node_modules/.cache/tsc/tsconfig.tsbuildinfo`.
  Re-running `pnpm typecheck` after a small edit skips unchanged
  files. `node_modules/.cache/` is already gitignored via the parent.
- **Vitest threads pool** in `vitest.config.ts`:
  `pool: "threads"`, `poolOptions.threads: { maxThreads: 4,
minThreads: 1 }`. Parallelizes test files; trivial today (2 test
  files) but compounds as the suite grows.
- **`.husky/pre-push`** narrows the test gate to
  `vitest run --changed origin/master`. Pushes only re-run tests
  related to the diff vs the trunk. Falls back to a full run if
  `origin/master` isn't available (fresh clone, etc.).

## Consequences

- A PR that only edits `README.md` or `docs/adr/*` shows just the
  `pr-rules.yml` jobs in the check list. Faster CI feedback and lower
  Actions usage.
- sccache caching is per-repo per-branch (GitHub Actions cache scope).
  First run on a new branch is a cache miss; subsequent runs hit. Cache
  storage is free on public repos; private repos have quota.
- Incremental tsc writes `*.tsbuildinfo` under `node_modules/.cache/`.
  Cleared on `pnpm install --frozen-lockfile` (node_modules is
  rewritten) so cold-start time is unchanged on CI.
- `vitest --changed origin/master` is a heuristic — it uses Vitest's
  module-graph analysis, which can miss tests if a dependency is
  reached only at runtime (e.g. dynamic imports it can't statically
  trace). CI runs the full suite, so any local false-negative is
  caught in PR before merge.
- `paths-ignore` in `pull_request` triggers a known GitHub quirk: when
  a required status check is configured for branch protection and the
  workflow is skipped, the check appears "pending" forever. Branch
  protection rules in this repo currently don't require these checks,
  so no impact today; revisit if/when branch protection is added.
