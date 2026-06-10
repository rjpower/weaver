#!/usr/bin/env python3
"""pr-label — flag sessions whose open PR lacks the loom label.

A builtin loom overlooker program (`builtin:pr-label`). Each round it surveys
the in-scope fleet over the loom REST API and, for every live session with an
open pull request, checks (via a `gh` read) that the PR carries the configured
label — so PRs born from loom sessions are identifiable on GitHub. Sessions
whose PR already has the label are skipped.

This builtin is **read-only**: it records a `would: label` action per
unlabelled PR and mutates nothing. When `gh` is unavailable (or the labels
can't be read) it still reports the ensure-label action, noting the labels
were unreadable.

Params: `{"label": "<name>"}` — the label to ensure; defaults to `weaver`.

Written against `weaver_loom`, the Python layer over the loom REST API — the
engine vendors that module onto PYTHONPATH for every script it runs. The
program contract (env in, one result JSON object on stdout) is documented in
docs/ARCHITECTURE.md (Overlookers).
"""

from weaver_loom import Round, gh

DEFAULT_LABEL = "weaver"


def pr_labels(repo_root, pr_number):
    """The PR's current label names, or None when unreadable."""
    reply = gh(["pr", "view", str(pr_number), "--json", "labels"], cwd=repo_root)
    if reply is None:
        return None
    return [label.get("name", "") for label in reply.get("labels") or []]


def main():
    rnd = Round()
    label = rnd.params.get("label") or DEFAULT_LABEL
    for session in rnd.sessions():
        branch = session.get("branch") or {}
        github = branch.get("github") or {}
        if github.get("pr_state") != "OPEN":
            continue
        pr = github.get("pr_number")
        labels = pr_labels(branch.get("repo_root"), pr)
        if labels is not None and label in labels:
            continue
        note = (
            f"PR #{pr} lacks label '{label}'"
            if labels is not None
            else f"PR #{pr}: labels unreadable via gh — would ensure '{label}'"
        )
        rnd.would("label", session=session["id"], pr=pr, label=label, note=note)
    rnd.finish(f"surveyed {rnd.surveyed}, {len(rnd.actions)} open PR(s) missing '{label}'")


if __name__ == "__main__":
    main()
