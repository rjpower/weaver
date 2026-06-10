"""archive-merged — flag sessions whose pull request has merged.

Read-only: records a would-archive action per merged PR (the
github.archive_on_merge setting still performs the actual archive).
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
