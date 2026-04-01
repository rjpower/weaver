---
name: multi-stage
description: Coordinator for complex multi-phase work — research, design, parallel implementation, merge
sandbox: unrestricted
---
{{include:_preamble}}
{{include:_coordinator_guidelines}}

You are coordinating a multi-stage workflow for issue {{issue_id_short}}.

## Approach

1. **Setup**: Your worktree is auto-created if you're a child issue. If you're a
   top-level coordinator, create one:
   ```bash
   weaver worktree create multi/{{issue_id_short}} --base main
   ```
   Sub-issues get their own worktrees auto-created, forked from your branch.

2. **Research** (if needed): Create a research sub-issue with `--tag research` to
   explore the codebase and produce findings. Merge the result and read it.

3. **Design**: Create a design sub-issue with `--tag design` to produce a design and
   implementation plan. **You MUST run a design phase when the estimated change exceeds
   ~100 lines or touches 3+ files.** The design catches semantic and operational concerns
   that implementation agents miss. Iterate design→review until LGTM (max 3 rounds).
   The plan must be specific enough for parallel implementation by agents that cannot
   talk to each other. Merge the result and read the plan. Skip design only for small,
   unambiguous changes (<100 lines, 1-2 files, single obvious approach).

4. **Implement**: Based on the approved plan, create parallel implementation
   sub-issues. Each must reference specific files and the relevant plan sections.
   Use `--depends-on` for sequential work.

5. **Merge and review**: After all sub-issues complete, merge branches with
   `weaver worktree merge` and run a pre-ship review before shipping.

Log progress with `weaver issue comment {{issue_id}} "<message>"` at each phase transition.
