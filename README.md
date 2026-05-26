# pstmacro

![CI](https://github.com/beyondsoft-kr/pstmacro/actions/workflows/ci.yml/badge.svg)
![Tauri build](https://github.com/beyondsoft-kr/pstmacro/actions/workflows/tauri-build.yml/badge.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

Tauri + React + TypeScript desktop app.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
(enforced by commitlint). Branch naming and Claude task automation are
documented in [CLAUDE.md](./CLAUDE.md).

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Scripts

```bash
pnpm dev              # Vite dev server
pnpm build            # tsc + Vite production build
pnpm tauri dev        # Tauri dev with desktop shell
pnpm tauri build      # Tauri release build

pnpm lint             # ESLint check
pnpm lint:fix         # ESLint autofix
pnpm lint:css         # Stylelint check
pnpm format           # Prettier write
pnpm format:check     # Prettier check (CI)
pnpm typecheck        # tsc --noEmit

pnpm test             # Vitest single run
pnpm test:watch       # Vitest watch
pnpm test:ui          # Vitest UI
pnpm test:coverage    # Vitest with coverage
```

`pnpm install` runs the `prepare` script which initializes Husky; the `.husky/_/` directory is auto-generated and gitignored by Husky.

### Rust (src-tauri)

```bash
cd src-tauri
cargo fmt              # format Rust sources
cargo clippy           # lint
```

## Commit message convention

Commits are validated by commitlint (`@commitlint/config-conventional`) via a
`commit-msg` hook. Use the [Conventional Commits](https://www.conventionalcommits.org/)
format:

```
<type>(<optional scope>): <subject>
```

Common types: `feat`, `fix`, `chore`, `refactor`, `docs`, `test`, `build`, `ci`, `perf`, `style`.

## VS Code

Opening the workspace in VS Code prompts to install the recommended extensions
(`.vscode/extensions.json`) — Prettier, ESLint, Stylelint, EditorConfig, Tauri,
rust-analyzer, Vitest Explorer. Workspace settings enable auto-save (1s delay),
format-on-save with Prettier, and ESLint/Stylelint autofix on save.
