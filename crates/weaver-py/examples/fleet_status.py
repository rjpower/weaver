#!/usr/bin/env python3
"""fleet_status — the acceptance demo for weaver_py.

Query the fleet and, capabilities permitting, mark a session. This is the
out-of-process overlooker pattern in miniature: a plain Python script driving
the loom REST API through the binding, with the capability gate enforced in the
binding so the script can never exceed its grant.

Run it against a running loom (set `$WEAVER_API`, or pass `--base`):

    pip install -e crates/weaver-py        # or: maturin develop
    python crates/weaver-py/examples/fleet_status.py
    python crates/weaver-py/examples/fleet_status.py --mark <session> attention "looks stuck"

With no `--mark`, it only observes (the `observe` capability, always on). Pass
`--mark` and the script requests the `mark` capability; running it without that
capability raises `CapabilityDenied` — the gate, demonstrated.
"""

import argparse
import sys

import weaver_py


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", default=None, help="loom base URL (default: $WEAVER_API or 127.0.0.1:7878)")
    ap.add_argument(
        "--mark",
        nargs=3,
        metavar=("SESSION", "LEVEL", "NOTE"),
        help="mark a session: <session> <ok|attention|blocked> <note>",
    )
    args = ap.parse_args()

    # Request only the capabilities this run needs: observe always, mark only
    # when actually marking. Least privilege, declared at construction.
    caps = ["mark"] if args.mark else []
    client = weaver_py.Client(base=args.base, capabilities=caps)
    print(f"# fleet @ {client.base}  (capabilities: {client.capabilities or ['observe']})")

    try:
        sessions = client.sessions()
    except weaver_py.WeaverError as e:
        print(f"could not reach loom: {e}", file=sys.stderr)
        return 1

    if not sessions:
        print("  (no active sessions)")
    for s in sessions:
        branch = s.get("branch", {})
        triage = branch.get("triage_level") or "-"
        print(
            f"  {s['id'][:8]}  {branch.get('title', ''):<30.30}  "
            f"attention={s['branch'].get('attention', '-'):<10}  triage={triage}"
        )

    if args.mark:
        session, level, note = args.mark
        try:
            updated = client.mark(session, level=level, note=note)
        except weaver_py.CapabilityDenied as e:
            print(f"refused: {e}", file=sys.stderr)
            return 2
        except weaver_py.WeaverError as e:
            print(f"mark failed: {e}", file=sys.stderr)
            return 1
        print(f"# marked {session} -> {updated['branch']['triage_level']} ({note!r})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
