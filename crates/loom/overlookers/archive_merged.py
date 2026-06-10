#!/usr/bin/env python3
"""archive-merged — flag sessions whose pull request has merged.

A builtin loom overlooker program (`builtin:archive-merged`). Each round it
surveys the in-scope fleet over the loom REST API and reports every live
session whose latest PR snapshot says the pull request is merged — work that is
integrated and only waiting for its session to be archived.

This builtin is **read-only**: it records a `would: archive` action per merged
session and mutates nothing. (Today the `github.archive_on_merge` setting
performs the actual archive; this program is the scripted home for that
workflow.)

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
import urllib.request

# Lifecycle states with no live session left to archive.
TERMINAL = {"done", "error", "archived"}


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


def main():
    cfg = json.loads(os.environ.get("WEAVER_OVERLOOKER", "{}"))
    scope = cfg.get("scope") or {}

    surveyed = 0
    actions = []
    for session in api_get("/sessions"):
        if session.get("status") in TERMINAL or not in_scope(scope, session):
            continue
        surveyed += 1
        github = (session.get("branch") or {}).get("github") or {}
        if github.get("pr_state") != "MERGED":
            continue
        pr = github.get("pr_number")
        actions.append(
            {
                "session": session["id"],
                "would": "archive",
                "pr": pr,
                "note": f"PR #{pr} is merged — the session can be archived",
            }
        )

    summary = f"surveyed {surveyed}, {len(actions)} with a merged PR"
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
