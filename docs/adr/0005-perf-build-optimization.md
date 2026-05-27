# 5. Build-time performance optimization

Date: 2026-05-27

## Status

Accepted

## Context

The Tauri release profile was Cargo defaults (`opt-level=3`, no LTO,
no strip), so the shipped binary carried debug symbols and unused
codegen. The Vite config only tuned the dev server — production
bundles kept `console.log`/`debugger`, no minify hints for Tauri's
webview targets, and Mantine wasn't pre-bundled so the first dev start
after `pnpm install` took noticeably longer. No bundle-size guardrail
either, so a heavy dep could quietly bloat the app.

## Decision

- **Cargo `[profile.release]`** in `src-tauri/Cargo.toml`:
  ```toml
  opt-level = "s"   # smaller binary; perf cost negligible for a UI shell
  lto = true
  codegen-units = 1
  strip = true
  panic = "abort"
  ```
  Chose `"s"` over `3`/`"z"` — `"s"` cuts ~30% size with almost no
  perf hit on typical UI workloads; `"z"` regresses CPU-bound paths,
  `3` keeps every symbol's full inline depth (largest binary).
- **Vite production** in `vite.config.ts`:
  - `build.target` resolved per `TAURI_ENV_PLATFORM` (`chrome105` on
    Windows, `safari14` elsewhere) — Tauri's bundled webview is
    modern, polyfills wasted.
  - `build.minify = "esbuild"` and `esbuild.drop = ["console",
"debugger"]` in non-debug builds (gated by `TAURI_ENV_DEBUG`).
  - `build.sourcemap = !!TAURI_ENV_DEBUG` so prod ships no maps.
  - `optimizeDeps.include = ["@mantine/core", "@mantine/hooks"]` —
    pre-bundle on cold start.
- **Size budget** via `size-limit`:
  - `.size-limit.json` caps JS at 300 KB and CSS at 100 KB
    (brotli-compressed; current usage ~70 KB / ~26 KB).
  - `.github/workflows/size-limit.yml` runs `andresz1/size-limit-action`
    on PRs targeting the default branch — posts a comment with
    delta and fails on budget breach.

## Consequences

- Release builds get slower (LTO + 1 codegen unit). Debug builds and
  `pnpm tauri dev` are unaffected.
- Production JS no longer carries `console.log` / `debugger`. Any
  diagnostics that should reach the user must use the project logger
  (`src/shared/lib/logger.ts`).
- A PR that drops in a fat dependency now visibly trips the size-limit
  comment. Raising the budget is a deliberate change reviewable in the
  same PR.
- `size-limit` reports brotli sizes — a fine proxy for disk size in
  Tauri's bundled webview context but not directly comparable to
  Vite's `gzip:` column.
