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

Written against `weaver_loom`, the Python layer over the loom REST API — the
engine vendors that module onto PYTHONPATH for every script it runs. The
program contract (env in, one result JSON object on stdout) is documented in
docs/ARCHITECTURE.md (Overlookers).
"""

from weaver_loom import Round


def main():
    rnd = Round()
    for session in rnd.sessions():
        github = (session.get("branch") or {}).get("github") or {}
        if github.get("pr_state") != "MERGED":
            continue
        pr = github.get("pr_number")
        rnd.would(
            "archive",
            session=session["id"],
            pr=pr,
            note=f"PR #{pr} is merged — the session can be archived",
        )
    rnd.finish(f"surveyed {rnd.surveyed}, {len(rnd.actions)} with a merged PR")


if __name__ == "__main__":
    main()
