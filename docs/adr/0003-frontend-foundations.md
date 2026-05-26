# 3. Frontend foundations — Error Boundary, logger, TanStack Query, Zustand

Date: 2026-05-26

## Status

Accepted

## Context

The frontend up to now has been raw React state in `Welcome` plus
ad-hoc `invoke`/`commands.greet` calls. Before the app grows we need
shared primitives for:

- **Catching unhandled render errors** so a thrown component doesn't
  blank the whole window.
- **Structured logging** that can later be piped to Rust `tracing` /
  Tauri log plugin without changing call sites.
- **Async server/Tauri-command state** (loading / error / retry /
  cache) without re-implementing the pattern per call.
- **Cross-feature client state** (e.g. theme, current user) that
  shouldn't live in component-local `useState`.

## Decision

- `react-error-boundary` wraps the app at root via `AppErrorBoundary`
  (`src/shared/ui/error-boundary.tsx`); the fallback shows the error
  message and a "Try again" reset button, and `onError` funnels into
  the project logger.
- A tiny `logger` (`src/shared/lib/logger.ts`) routes `debug | info |
warn | error` to the matching `console` methods with a `[level]`
  prefix. Replacing the implementation later (Tauri command, OTel,
  Sentry) is a one-file change.
- `@tanstack/react-query` handles all server/Tauri-command state.
  `AppProviders` (`src/app/providers.tsx`) wires `QueryClientProvider`
  with desktop-friendly defaults (`retry: 1`,
  `refetchOnWindowFocus: false`). `Welcome.greet` is migrated to
  `useMutation(commands.greet)`.
- `zustand` is installed for future global client state. No store is
  defined yet — adding one is `src/shared/lib/store.ts` (or, when
  feature-local, `src/features/<name>/store.ts`).

## Consequences

- All new async Tauri-command calls should be wrapped in `useQuery` /
  `useMutation` rather than bare `await commands.X(...)`, so loading
  and error states are handled consistently.
- Component tests rendering anything that uses Query hooks must wrap
  with a fresh `QueryClientProvider` (see `welcome.test.tsx`
  `renderWithQuery`); the providers in `AppProviders` are not active
  in tests by default.
- ReactQueryDevtools is intentionally not wired yet — add when there's
  enough query traffic to want the panel; gate behind
  `import.meta.env.DEV`.
- Zustand adds <1KB to the bundle even unused — the cost of having it
  ready is negligible.
