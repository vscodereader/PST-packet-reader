#!/usr/bin/env bash
# TaskCreated hook: create a GitHub issue mirroring the new Claude task.
# Reads the hook payload from stdin and uses `gh` to create an issue.
# Stores the task id inside the issue body as an HTML comment so
# task-complete-to-pr.sh can find the matching issue later.

set -euo pipefail

PAYLOAD=$(cat)

extract() {
  echo "$PAYLOAD" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    d = {}
def get(path):
    cur = d
    for k in path.split('.'):
        if isinstance(cur, dict) and k in cur:
            cur = cur[k]
        else:
            return None
    return cur
for path in [$1]:
    v = get(path)
    if v is not None and v != '':
        print(v)
        sys.exit(0)
print('')
"
}

SUBJECT=$(extract "'task.subject', 'subject', 'tool_input.subject'")
[ -z "$SUBJECT" ] && SUBJECT="Untitled Claude task"
DESCRIPTION=$(extract "'task.description', 'description', 'tool_input.description'")
TASK_ID=$(extract "'task.id', 'task_id', 'id', 'tool_input.id'")

# Trunk-based policy: only feat-flavored work warrants an issue + PR.
# chore/fix/refactor/docs/style/test/perf/ci/build land as commit-level
# PRs (no issue link required). Skip issue creation for them.
shopt -s nocasematch
case "$SUBJECT" in
  feat:*|feat\(*|feature:*|"feat "*|"feature "*) ;;  # proceed
  fix:*|fix\(*|"fix "*) exit 0 ;;
  chore:*|chore\(*|"chore "*) exit 0 ;;
  refactor:*|refactor\(*|"refactor "*) exit 0 ;;
  docs:*|docs\(*|"docs "*) exit 0 ;;
  style:*|style\(*|"style "*) exit 0 ;;
  test:*|test\(*|"test "*) exit 0 ;;
  perf:*|perf\(*|"perf "*) exit 0 ;;
  ci:*|ci\(*|"ci "*) exit 0 ;;
  build:*|build\(*|"build "*) exit 0 ;;
  *) ;;  # untyped tasks: default to creating an issue
esac
shopt -u nocasematch

# Skip silently if gh is missing or unauthenticated.
if ! command -v gh >/dev/null 2>&1; then exit 0; fi
if ! gh auth status >/dev/null 2>&1; then exit 0; fi
if ! gh repo view >/dev/null 2>&1; then exit 0; fi

BODY="${DESCRIPTION}

---
<!-- claude-task-id: ${TASK_ID} -->
*Auto-created by Claude TaskCreated hook.*"

gh issue create \
  --title "$SUBJECT" \
  --body "$BODY" \
  --label "claude-task,parallel-safe" \
  >/dev/null 2>&1 || true
