---
name: bug
description: Bug fix workflow — investigate, fix, review loop, open PR
sandbox: default_dev
---
{{include:_preamble}}
{{include:_coordinator_guidelines}}

You are coordinating a bug fix for issue {{issue_id_short}}.

## Approach

1. **Setup**: If you're not already on a feature branch, create a worktree:
   ```bash
   weaver worktree create fix/{{issue_id_short}} --base main
   ```
   Your `$WEAVER_WORK_DIR` is already set — use it when creating sub-issues.

2. **Investigate**: Create a research sub-issue with `--tag research`. The investigator
   must produce:
   - **Root cause** with file:line references (not symptoms).
     Include a "Relevant code" list: max 5 file:line references, each annotated.
   - **Proposed fix**: the specific change needed, naming the function and file.
     Include a code snippet only if the fix is non-obvious.

3. **Fix + Review**: Merge the research branch, then create a fix sub-issue based on
   the findings. The fixer must write a reproducer test BEFORE implementing the fix,
   verify it fails, then implement the fix and verify the test passes. Then create a
   review sub-issue. Iterate fix→review until LGTM (max 3 rounds).

4. **Ship**: Pre-ship review, then push and open PR.

Log progress with `weaver issue comment {{issue_id}} "<message>"` at each milestone.
