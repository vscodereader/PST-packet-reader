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

## Branch & PR workflow (Trunk-Based Development)

This project follows **Trunk-Based Development**: small, frequent commits
land on `master` through short-lived branches. Long-lived feature
branches are an anti-pattern here.

**Master is protected.** Direct pushes to `master`/`main` are blocked by
the `.husky/pre-push` hook. Every change still goes through a branch +
PR — the difference is how heavyweight that ceremony is.

### Two work tracks

**1. `feat/<issue>` — full ceremony** (new user-facing behavior, anything
non-trivial):

1. `gh issue create ...` (or the TaskCreated hook does it).
2. `git checkout -b feat/<issue-number>` off the latest `master`.
3. TDD-first: write a failing test, then the code that makes it pass.
   Pre-commit (`lint-staged`), `commit-msg` (commitlint), and pre-push
   (`vitest run`) gate each commit / push.
4. PR with `Closes #<issue>` in the body — the rules below apply.

**2. `<type>/<short-slug>` — commit-level work** (chore, fix, refactor,
docs, style, test, perf, ci, build):

1. **No issue required.** Branch off `master` with a descriptive slug
   (`chore/bump-deps`, `fix/login-typo`, `refactor/extract-helper`).
2. Keep it to one logical change, ideally one commit. Same hook gates
   apply (pre-commit / commit-msg / pre-push).
3. Push and open a PR. Mark it ready and auto-merge:
   `gh pr merge --auto --squash --delete-branch`. No review wait, no
   issue link required.

If a chore / fix is the natural counterpart to in-flight feat work,
commit it directly on the feat branch instead of spinning a separate
branch — that's the simplest path.

### Multi-task parallelism (worktrees)

When two or more tasks can proceed in parallel without shared file
edits, dispatch them in separate worktrees so they don't trample each
other's working tree. `.claude/settings.json` is configured for this:
`baseRef: fresh`, `symlinkDirectories: [node_modules, .husky/_]`,
`bgIsolation: worktree`. Use `Agent` with `isolation: "worktree"` for
write-capable subagents, or `git worktree add` manually for hand-run
parallelism (see the Worktrees section above).

### CI-enforced rules (`.github/workflows/pr-rules.yml`)

- Branch name must match `^(feat|fix|chore|refactor|docs|test|build|ci|perf|style)/(\d+|[a-z][a-z0-9-]*)$` (either an issue number for feat, or a slug for the other types).
- PR title must be a conventional commit form (`type(scope?): subject`).
- For `feat/<n>` branches only: PR body must contain `Closes #<n>` (or
  `Fixes`/`Resolves`). Other branch types are exempt.

A PR failing any rule applicable to its branch type is non-mergeable.

If a task is purely investigative (no code change), the TaskCompleted
hook will skip PR creation; for a feat task the issue stays open until
closed manually or via a later PR.

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
