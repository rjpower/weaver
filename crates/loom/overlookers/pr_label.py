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

The program contract (shared by every script the engine runs):

* `WEAVER_API` — base URL of the loom REST API (e.g. `http://127.0.0.1:7878`).
* `WEAVER_OVERLOOKER` — JSON config for this round:
  `{id, name, program, params, scope, capabilities, dry_run}`.
* Print one JSON object to stdout: `{"outcome": "ok"|"noop", "summary": str,
  "actions": [...]}`. A non-zero exit (or unparseable stdout) records the
  round as an error.
"""

import json
import os
import subprocess
import urllib.request

# Lifecycle states with no live session behind the PR.
TERMINAL = {"done", "error", "archived"}

DEFAULT_LABEL = "weaver"


def api_get(path):
    base = os.environ.get("WEAVER_API", "http://127.0.0.1:7878").rstrip("/")
    if not base.startswith("http"):
        base = "http://" + base
    with urllib.request.urlopen(base + "/api" + path) as resp:
        return json.load(resp)


def attention_of(branch):
    """The branch's `attention` tag value; absence is the calm `ok`."""
    for tag in branch.get("tags") or []:
        if tag.get("key") == "attention":
            return tag.get("value") or "ok"
    return "ok"


def in_scope(scope, session):
    """Apply the overlooker's fleet query: an `attention` filter (`!ok` or an
    exact level) and an optional `repo` pin."""
    branch = session.get("branch") or {}
    want = scope.get("attention")
    if want:
        have = attention_of(branch)
        matched = have != want[1:] if want.startswith("!") else have == want
        if not matched:
            return False
    repo = scope.get("repo")
    if repo and branch.get("repo_root") != repo:
        return False
    return True


def pr_labels(repo_root, pr_number):
    """The PR's current label names via a `gh` read, or None when unreadable
    (gh missing, unauthenticated, no GitHub remote, ...)."""
    try:
        out = subprocess.run(
            ["gh", "pr", "view", str(pr_number), "--json", "labels"],
            cwd=repo_root,
            capture_output=True,
            text=True,
            timeout=30,
        )
        if out.returncode != 0:
            return None
        labels = json.loads(out.stdout).get("labels") or []
        return [label.get("name", "") for label in labels]
    except (OSError, subprocess.SubprocessError, ValueError):
        return None


def main():
    cfg = json.loads(os.environ.get("WEAVER_OVERLOOKER", "{}"))
    scope = cfg.get("scope") or {}
    label = (cfg.get("params") or {}).get("label") or DEFAULT_LABEL

    surveyed = 0
    actions = []
    for session in api_get("/sessions"):
        if session.get("status") in TERMINAL or not in_scope(scope, session):
            continue
        surveyed += 1
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
        actions.append(
            {
                "session": session["id"],
                "would": "label",
                "pr": pr,
                "label": label,
                "note": note,
            }
        )

    summary = f"surveyed {surveyed}, {len(actions)} open PR(s) missing '{label}'"
    print(
        json.dumps(
            {
                "outcome": "ok" if actions else "noop",
                "summary": summary,
                "actions": actions,
            }
        )
    )


if __name__ == "__main__":
    main()
