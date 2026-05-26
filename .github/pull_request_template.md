## Summary

<!-- 1–3 bullets describing what changed and why -->

Closes #

## Test plan

<!-- Bulleted checklist. Local commands + what CI should verify. -->

- [ ] `pnpm format:check && pnpm lint && pnpm lint:css && pnpm typecheck && pnpm test`
- [ ] `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings`
- [ ] CI green (frontend + rust + tauri-build)
