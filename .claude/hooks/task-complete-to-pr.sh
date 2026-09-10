#!/usr/bin/env bash
# TaskCompleted hook: when a Claude task is marked complete, try to attach
# the work on the current feature branch to a PR that closes the matching
# issue. Best-effort — silent failure is preferred over breaking the task
# completion flow.

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

TASK_ID=$(extract "'task.id', 'task_id', 'id', 'tool_input.id'")

if ! command -v gh >/dev/null 2>&1; then exit 0; fi
if ! gh auth status >/dev/null 2>&1; then exit 0; fi
if ! gh repo view >/dev/null 2>&1; then exit 0; fi

# Find the issue created for this task via the embedded HTML comment.
ISSUE_NUM=""
if [ -n "$TASK_ID" ]; then
  ISSUE_NUM=$(gh issue list \
    --label claude-task \
    --state open \
    --search "in:body \"claude-task-id: ${TASK_ID}\"" \
    --json number --jq '.[0].number // ""' 2>/dev/null || echo "")
fi

if [ -z "$ISSUE_NUM" ]; then
  exit 0
fi

BRANCH=$(git branch --show-current 2>/dev/null || echo "")
DEFAULT_BRANCH=$(gh repo view --json defaultBranchRef --jq '.defaultBranchRef.name' 2>/dev/null || echo "main")

# Only act on feature branches with commits ahead of the default branch.
if [ -z "$BRANCH" ] || [ "$BRANCH" = "$DEFAULT_BRANCH" ]; then
  exit 0
fi

if ! git rev-parse --verify "origin/${DEFAULT_BRANCH}" >/dev/null 2>&1; then
  exit 0
fi

AHEAD=$(git rev-list --count "origin/${DEFAULT_BRANCH}..HEAD" 2>/dev/null || echo "0")
if [ "$AHEAD" = "0" ]; then
  exit 0
fi

# Push the branch (no-op if already up to date).
git push -u origin "$BRANCH" >/dev/null 2>&1 || true

# If a PR already exists for this branch, append the closer once.
EXISTING_PR=$(gh pr view --json number --jq '.number' 2>/dev/null || echo "")
if [ -n "$EXISTING_PR" ]; then
  CURRENT_BODY=$(gh pr view --json body --jq '.body' 2>/dev/null || echo "")
  if ! echo "$CURRENT_BODY" | grep -q "Closes #${ISSUE_NUM}\b"; then
    gh pr edit --body "${CURRENT_BODY}

Closes #${ISSUE_NUM}" >/dev/null 2>&1 || true
  fi
  exit 0
fi

# No PR yet — create one. Title taken from the most recent commit subject.
PR_TITLE=$(git log -1 --pretty=%s 2>/dev/null || echo "Claude task ${TASK_ID}")
gh pr create \
  --title "$PR_TITLE" \
  --body "Closes #${ISSUE_NUM}" \
  --base "$DEFAULT_BRANCH" \
  >/dev/null 2>&1 || true
