# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

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
