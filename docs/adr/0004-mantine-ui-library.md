# 4. Mantine as the UI component library

Date: 2026-05-26

## Status

Accepted

## Context

After the folder-layout refactor (`src/features/<name>/`, `src/shared/ui/`)
landed, the welcome screen was still raw HTML + a hand-rolled CSS file.
Going forward we need a consistent set of UI primitives (typography,
forms, layout, feedback) with sensible defaults, accessibility built in,
and a theming story that doesn't require us to maintain a design system
from scratch.

Considered: shadcn/Radix (great primitives, but every component is
hand-installed and we'd own all the styling), Chakra (heavier runtime
than we need), MUI (good but stylistically opinionated and heavy),
Mantine (broad component set, hooks package, CSS modules, light/dark
out of the box, mature ecosystem).

## Decision

- Adopt **Mantine** (`@mantine/core` + `@mantine/hooks`, v9) as the
  primary UI component library.
- Configure via the official Vite postcss setup:
  `postcss-preset-mantine` + `postcss-simple-vars` with the standard
  breakpoint variables.
- A single `src/app/providers.tsx` wires `<MantineProvider
defaultColorScheme="auto">` so the OS preference drives the theme.
  When the frontend-foundations work (#21) lands, this Provider
  composes with `QueryClientProvider` and `AppErrorBoundary` — order
  Mantine outermost so toasts/modals from other libs (or future
  Mantine modals/notifications) can mount within Mantine's
  CSS context.
- Migrate the existing welcome screen to Mantine primitives
  (`Container`, `Title`, `Group`, `Anchor`, `TextInput`, `Button`,
  `Image`, `Text`, `Stack`) and delete `welcome.css`.

## Consequences

- Future feature screens build on Mantine primitives; no more bespoke
  CSS for layout/typography. Custom styles live in CSS Modules per
  component when needed (Mantine plays well with them).
- Bundle size grows by Mantine core (~150KB gzipped pre-treeshake — tree
  shakes well). Acceptable for a Tauri desktop bundle.
- Tests that render Mantine components must wrap in `<MantineProvider>`
  — see `features/welcome/welcome.test.tsx` `renderWithMantine`.
- Light/dark toggle UI is a separate task; auto-mode handles passive
  switching from the OS today.
- Adding additional Mantine packages (`@mantine/form`,
  `@mantine/notifications`, `@mantine/dates`, `@mantine/modals`,
  `@mantine/charts`) is deferred until first use site — each requires
  its own styles import and (sometimes) Provider.
