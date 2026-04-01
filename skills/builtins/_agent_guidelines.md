## Guidelines

### Worktree by default

Your working directory (`$WEAVER_WORK_DIR`) is set automatically. Child issues get
their own worktree auto-created by the executor — you don't need to create one.
If you're a top-level issue on `main`, create a worktree first:

```bash
weaver worktree create work/$WEAVER_ISSUE_ID --base main
```

### Progress updates

Report progress so the user can follow along in the dashboard:

```bash
weaver issue comment $WEAVER_ISSUE_ID "investigating the auth module..."
weaver issue comment $WEAVER_ISSUE_ID "found root cause: missing null check in validate()"
weaver issue comment $WEAVER_ISSUE_ID "fix applied, running tests..."
```

Do this at natural milestones: when you start investigating, when you find something significant, before/after running tests. Aim for a comment every few minutes of work.

### Commits

Uncommitted changes are auto-committed when you finish (success or failure), so nothing
is ever lost. But you should still commit at logical checkpoints for clearer git history.

Sub-issues get their own worktree auto-created by the executor, forked from your
branch. They can only see **committed** work — uncommitted files in your worktree are
invisible to children. Always commit your changes before creating sub-issues that
depend on them. If a sub-issue should share your worktree instead of getting its own,
pass `--same-worktree` when creating it.

### Decomposition

When your task involves **3 or more independent work items** (changes to unrelated files/modules, independent features, etc.), decompose into sub-issues instead of doing everything yourself:

1. **Assess**: Can the work items be done independently? Do they touch different files?
2. **Create sub-issues** with `--parent $WEAVER_ISSUE_ID` and `--tag step`
3. **Express parallelism**: items that can run concurrently get NO `--depends-on` between them. Items that must be sequential chain `--depends-on`.
4. **Wait**: `weaver issue wait-all <id1> <id2> ...`
5. **Merge**: `weaver worktree merge <id1> <id2> ...`
6. **Verify**: run the full test suite on the merged result

Each sub-issue body must be specific: name the files to change, the behavior to implement, and the tests to write. Do NOT create sub-issues with vague descriptions like "implement the database layer."

Do NOT decompose trivially small tasks. If the entire change is <50 lines across 1-2 files, just do it directly.

### Timeouts

Always set timeouts on external commands that could hang. A stuck test or build
wastes your entire issue budget:

- **Tests**: `timeout 120 uv run pytest ...` or `--timeout 60` (pytest flag)
- **Builds**: `timeout 300 cargo build ...`
- **Bash tool**: use the `timeout` field (milliseconds) — `"timeout": 120000`
- **Network**: `curl --max-time 30`, `wget --timeout=30`

If a command times out, diagnose (deadlock? missing dep? infinite loop?) rather
than retrying with a longer timeout.

### General

- If you hit a blocker you cannot resolve, describe it clearly in your output.
- Focus on your assigned task and produce concrete, working code.
- When creating sub-issues, set `--parent` to your issue ID so the DAG is tracked.
- Use `--depends-on` to express ordering between sub-issues.

## Before You Start

1. Read `AGENTS.md` in the repo root. Follow its guidelines for all work.
2. Check for existing work on this topic:
   ```bash
   gh pr list --state open --search "<keywords from issue title>"
   gh issue list --state open --search "<keywords from issue title>"
   ```
   If overlapping work exists, do not duplicate it. Comment and coordinate.

## Writing Style

All output (comments, PR descriptions, commit messages) must be terse:
- No preamble or filler ("I've thoroughly investigated...", "After careful analysis...")
- No repeating information already in the issue body
- No restating what code does when a file:line link suffices
- Max 3-4 sentences of prose per section; use code references, not words
- Never credit yourself in commits or comments

## Definition of Done

An issue is only complete when:
- All tests pass (run the test commands from AGENTS.md)
- Code compiles without warnings
- Changes are committed with a clear commit message
