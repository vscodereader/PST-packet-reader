# Project conventions for Claude

This project mirrors Claude's task list to GitHub Issues and PRs via hooks in
`.claude/settings.json`. Follow the workflow below so the automation works as
intended.

## Task granularity

Break work into **micro-tasks** — one TaskCreate call per atomic unit. A good
rule of thumb: if a task takes more than ~5 logical steps or touches more than
one concern, split it. Each TaskCreate becomes one GitHub Issue, so smaller
tasks produce a cleaner backlog and a more reviewable PR diff.

## Parallel-safe by default

Issues created by the TaskCreated hook are labeled `parallel-safe`. If a task
depends on another, express it explicitly via `TaskUpdate addBlockedBy: [...]`.
Tasks without `addBlockedBy` may be dispatched in parallel — use the
`superpowers:dispatching-parallel-agents` skill when 2+ independent tasks
exist.

## Worktrees for parallel work

`.claude/settings.json` is configured to give each parallel agent its own
git worktree:

- `worktree.baseRef = "fresh"` — branches each worktree from
  `origin/<default>` (clean slate, no in-flight changes).
- `worktree.symlinkDirectories = ["node_modules", ".husky/_"]` — shares the
  installed deps and the Husky wrapper directory across worktrees so
  spawning a new one is near-instant (no `pnpm install` per worktree, and
  the commit hooks fire immediately).
- `worktree.bgIsolation = "worktree"` — background sessions get an
  isolated worktree by default so they can't edit the main checkout while
  you are working in it.

**When dispatching parallel agents**, prefer the `Agent` tool with
`isolation: "worktree"` for any agent that will edit files. Read-only
exploration agents don't need isolation.

For manual experimentation:

```bash
# Spawn a worktree at a sibling path for an issue branch
git worktree add ../pstmacro-feat-3 -b feat/3
ln -s "$(pwd)/node_modules" ../pstmacro-feat-3/node_modules
ln -s "$(pwd)/.husky/_"     ../pstmacro-feat-3/.husky/_
# ...work there...
git worktree remove ../pstmacro-feat-3   # when done
```

Do **not** symlink `src-tauri/target` or `dist` — concurrent Cargo / Vite
builds on a shared output directory corrupt each other.

## Branch & PR workflow

**Master is protected.** Direct pushes to `master`/`main` are blocked by the
`.husky/pre-push` hook. All changes reach the default branch via pull
request.

**Branch naming**: `<type>/<issue-number>` where `<type>` is one of `feat`,
`fix`, `chore`, `refactor`, `docs`, `test` (matches the commitlint types).
Examples: `feat/12`, `fix/47`, `chore/3`. One issue per branch keeps the PR
focused.

**Per-task workflow**:

1. `gh issue create ...` (or rely on the TaskCreated hook to create one
   automatically).
2. `git checkout -b feat/<issue-number>` off the latest `master`.
3. Commit work on that branch. The Husky `pre-commit` hook runs
   lint-staged; `commit-msg` validates the conventional commit format.
4. When done, the TaskCompleted hook pushes the branch and opens (or
   updates) a PR whose body contains `Closes #<issue>`, so merging the PR
   closes the issue.

**These rules are CI-enforced** by `.github/workflows/pr-rules.yml`:

- Branch name must match `^(feat|fix|chore|refactor|docs|test|build|ci|perf|style)/\d+$`.
- PR title must be a conventional commit form (`type(scope?): subject`).
- PR body must contain `Closes #<n>` (or `Fixes`/`Resolves`).

A PR failing any of the three is non-mergeable.

If a task is purely investigative (no code change), the TaskCompleted hook
will skip PR creation; the issue remains open until you close it manually
or reference it from another PR.

## TDD enforcement

- **Coverage gate**: `pnpm test:coverage` (run in CI) fails when any of
  lines / statements / functions / branches drops below **70%**. The
  config is in `vitest.config.ts` under `test.coverage`.
- **Pre-push tests**: `.husky/pre-push` runs `vitest run --silent` and
  blocks the push on any failure.
- **New-file-needs-test**: `.github/workflows/test-required.yml` checks
  the PR diff. Any added `src/**/*.{ts,tsx}` (excluding `*.test.*`,
  `src/test/**`, `src/main.tsx`, `src/vite-env.d.ts`) must have an
  adjacent `*.test.{ts,tsx}` or the PR can't merge.
- **Test-file lint**: `@vitest/eslint-plugin` rules enforce
  `no-disabled-tests`, `no-focused-tests`, `require-top-level-describe`
  on `**/*.test.{ts,tsx}` and `src/test/**`.
## Architecture & code style enforcement

- **Import order**: `eslint-plugin-import` enforces groups (builtin →
  external → internal → parent → sibling → index) with alphabetized
  members and a blank line between groups. Auto-fixable.
- **No duplicate / dead imports**: `import/no-duplicates` +
  `unused-imports/no-unused-imports`.
- **TypeScript strictness**: `tsconfig.json` enables
  `noUncheckedIndexedAccess` (array/dict access yields `T | undefined`)
  and `exactOptionalPropertyTypes` (`{ x?: T }` ≠ `{ x: T | undefined }`).
  Tighter than `strict: true` alone — guard accordingly.

## Manual operations

| What                              | Command                                                               |
| --------------------------------- | --------------------------------------------------------------------- | --- |
| List Claude-created issues        | `gh issue list --label claude-task`                                   |
| Close an issue by hand            | `gh issue close <n>`                                                  |
| Disable the automation            | Delete or rename `.claude/settings.json`                              |
| Inspect hook payload during debug | Temporarily prefix the script with `tee /tmp/claude-task-payload.json | `   |

## Caveats

- **Issue volume**: every TaskCreate creates an issue. Don't use TaskCreate for
  trivial bookkeeping — only for actual work units.
- **Title fuzziness**: the hook matches issues by an embedded HTML comment
  (`<!-- claude-task-id: N -->`). Editing or deleting that comment breaks the
  TaskCompleted linkage.
- **PR-per-task is not strict**: if multiple tasks complete on the same branch,
  the first one creates the PR and subsequent completions append additional
  `Closes #N` lines to the PR body.
