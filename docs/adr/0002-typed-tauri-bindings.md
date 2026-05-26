# 2. Typed Rust↔TS bindings via tauri-specta

Date: 2026-05-26

## Status

Accepted

## Context

The Tauri command bridge is `invoke(name: string, args: object) -> Promise<unknown>`. Frontend call sites must remember the command name as a string literal and the exact argument shape, with no compile-time check that they match the Rust side. As we add commands and structs this divergence is a frequent source of runtime errors.

## Decision

Use [`tauri-specta`](https://github.com/oscartbeaumont/tauri-specta) (v2.0.0-rc) plus [`specta`](https://docs.rs/specta) + `specta-typescript` to emit a typed `commands.ts` from the Rust command signatures into `src/shared/bindings/commands.ts`.

- Each `#[tauri::command]` gets an additional `#[specta::specta]` attribute.
- A shared `make_builder()` in `src-tauri/src/lib.rs` wires `collect_commands![...]` for both the runtime entry point and the export test.
- `src-tauri/tests/export_bindings.rs` is the export trigger: `cargo test --test export_bindings` runs the builder's `.export()`, then prepends `// @ts-nocheck` + `/* eslint-disable */` to the generated file so it cleanly passes our other tooling.
- `package.json` exposes `pnpm tauri:bindings` as the human entry point.
- The generated file is committed (CI/PR review visibility) and ignored by Prettier and ESLint; tsc skips it via the `// @ts-nocheck` header. Edits to it are auto-overwritten by regeneration.

## Consequences

- Call sites become `commands.greet(name)` with full types; renaming a command, changing an arg, or returning a new type is a compile-time error frontend-side.
- New commands must remember `#[specta::specta]` AND be added to `collect_commands![...]`. A missing entry produces a TS-level "property does not exist" on next regeneration.
- Bindings drift if `pnpm tauri:bindings` is forgotten. CI runs `cargo test` (which includes `export_bindings`), so any drift fails CI on the PR that introduced it; locally, run it before committing changed Rust commands.
- We are pinned to a rc release (`tauri-specta 2.0.0-rc.21`) until the crate stabilizes. Bump together with `specta` / `specta-typescript` to avoid breaking-API mismatches.
