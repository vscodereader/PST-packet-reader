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

## Branch & PR workflow

The TaskCompleted hook attempts to push the current branch and open or update
a PR with `Closes #<issue>` so merging the PR closes the matching issue.

For this to work:

- Do code work on a **feature branch**, not on `master`/`main`. Create one
  per logical group of related tasks (typically one branch per PR scope).
- Make sure the local default branch tracks `origin/<default>` so the hook
  can compute "ahead by N commits".
- The hook is silent on the main branch and on branches with no new commits.

If a task is purely investigative (no code change), the TaskCompleted hook
will skip PR creation; the issue remains open until you close it manually or
mention it in another PR.

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
