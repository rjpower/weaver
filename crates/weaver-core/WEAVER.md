You are running inside a **weaver session**: a detached agent workstream in a
git worktree on its own branch. The user is not watching this terminal — they
review progress asynchronously through the loom dashboard.

This document describes how to work *with weaver*. It is distinct from any
`AGENTS.md` in the repo, which describes the project itself — read that too.

## The `weaver` CLI

It is on your `PATH`. From anywhere in the worktree you can run:

- `weaver goal` — print the task this branch was created for.
- `weaver note "<text>"` — append a progress note, or a decision and its
  rationale, to the branch log.
- `weaver describe "<text>"` — set the one-line summary shown on the dashboard.
- `weaver status <level> "<note>"` — tell the dashboard how you are doing.
  `level` is one of `ok`, `attention`, or `blocked`; the note is a short reason.
- `weaver issue add "<title>"` — add a task claimed by this branch (`--repo`
  files it in the shared repo backlog instead).
- `weaver issue ls` — this branch's tasks plus the unclaimed repo backlog
  (`--mine` for just yours, `--repo` for the whole repo). `weaver issue close N`.
- `weaver status` — with no argument, show the goal, attention, open-issue
  count, and latest summary.

These talk directly to the weaver database — no daemon required.

## Signalling your status

The user scans the dashboard for sessions that need them. Keep your attention
level honest with `weaver status <level> "<reason>"`:

- `ok` — progressing normally, **or** blocked on something external that is not
  the user (a CI run, a PR review, a long workflow). No action needed.
  Example: `weaver status ok "waiting on PR review feedback"`.
- `attention` — you want the user to look: a question, a decision to confirm, or
  "done — ready for review". Example: `weaver status attention "ready for review"`.
- `blocked` — you are stuck or hit an error and need help to proceed.
  Example: `weaver status blocked "build broken, can't reproduce locally"`.

Set it as your situation changes — especially raise it to `attention` before you
finish a turn expecting the user, and drop back to `ok` once you are moving
again. This replaces the old guessed working/waiting/idle indicator, which was
often wrong (e.g. it read "idle" while you were really waiting on a workflow).

## How to work here

- Prefer to make a well-reasoned decision, record it with `weaver note`, and
  keep going. Default to recording and continuing rather than stopping.
- You may still ask the user for feedback in plain prose when a choice genuinely
  matters. But **never block on an interactive TUI prompt** — multiple-choice
  menus, plan-approval dialogs, and the like cannot be answered from the
  dashboard. State the question as plain text, record it with `weaver note`,
  set `weaver status attention "<the question>"`, and continue with your best
  assumption.

## Finishing work

You are on a dedicated branch in your own worktree. There is no "merge" button —
integrating your work back is a deliberate act, and it is yours to drive. When
the work is ready:

- **Commit** your changes with a clear message and keep the worktree clean.
- **Run the project's checks** — formatters, linters, pre-commit hooks, and the
  test suite — and make them pass before you call the work done. If the repo
  documents specific commands (often in `AGENTS.md`), use those.
- **Open a pull request** rather than merging into the base branch yourself,
  unless the user has told you otherwise. Most teams gate integration on review
  and CI; respect that. Use the project's normal PR workflow (e.g. `gh pr
  create`).
- Record what you did and any follow-ups with `weaver note`, set a concise
  summary with `weaver describe`, and raise `weaver status attention "ready for
  review"` so the user knows to look.

When a session is finished with, the user may **archive** it from the dashboard:
that tears down this tmux session and removes the worktree, but preserves the
branch and the weaver history (goal, notes, summaries) for future reference. So
make sure anything worth keeping is committed to the branch or recorded as a
note before you consider the task complete.
