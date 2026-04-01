#!/usr/bin/env python3
"""Programmable mock agent for Playwright E2E tests.

Reads a JSON program file and outputs NDJSON to stdout, simulating
the claude --output-format stream-json output format.

Program resolution:
  $MOCK_PROGRAMS_DIR/$WEAVER_ISSUE_ID.json
  $MOCK_PROGRAMS_DIR/_default.json
  Built-in default (init + result)

Environment variables:
  MOCK_PROGRAMS_DIR  - directory containing program JSON files
  WEAVER_ISSUE_ID    - issue ID (set by weaver executor)
  WEAVER_API_URL     - API URL (set by weaver executor, used for create_issue)
  WEAVER_BINARY_PATH - path to weaver binary (used for review_request)
  WEAVER_DB_PATH     - path to weaver DB (used for review_request)
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.request


def emit(obj: dict) -> None:
    print(json.dumps(obj), flush=True)


def emit_init(session_id: str = "mock-session", model: str = "mock-model") -> None:
    emit({
        "type": "system",
        "subtype": "init",
        "session_id": session_id,
        "model": model,
        "tools": [],
    })


def emit_result(
    result: str = "done",
    model: str = "mock-model",
    input_tokens: int = 100,
    output_tokens: int = 50,
    cost_usd: float = 0.01,
) -> None:
    emit({
        "type": "result",
        "subtype": "success",
        "is_error": False,
        "result": result,
        "session_id": "mock-session",
        "model": model,
        "total_cost_usd": cost_usd,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        },
    })


def execute_step(step: dict) -> None:
    action = step["action"]

    if action == "init":
        emit_init(
            step.get("session_id", "mock-session"),
            step.get("model", "mock-model"),
        )

    elif action == "text":
        emit({
            "type": "assistant",
            "message": {
                "content": [{"type": "text", "text": step["text"]}],
            },
        })

    elif action == "tool_use":
        emit({
            "type": "assistant",
            "message": {
                "content": [{
                    "type": "tool_use",
                    "name": step["tool"],
                    "id": step.get("id", "call_1"),
                    "input": step.get("input", {}),
                }],
            },
        })

    elif action == "tool_result":
        emit({
            "type": "assistant",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": step.get("tool_use_id", "call_1"),
                    "content": step.get("output", ""),
                }],
            },
        })

    elif action == "sleep":
        time.sleep(step["ms"] / 1000.0)

    elif action == "create_issue":
        api_url = os.environ.get("WEAVER_API_URL", "http://localhost:8080")
        parent_id = os.environ.get("WEAVER_ISSUE_ID", "")
        payload = {
            "title": step["title"],
            "body": step.get("body", ""),
            "tags": step.get("tags", []),
        }
        if step.get("as_child", True) and parent_id:
            payload["parent_issue_id"] = parent_id
        data = json.dumps(payload).encode()
        req = urllib.request.Request(
            f"{api_url}/api/issues",
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        urllib.request.urlopen(req)

    elif action == "review_request":
        weaver_bin = os.environ.get("WEAVER_BINARY_PATH", "weaver")
        db_path = os.environ.get("WEAVER_DB_PATH", "")
        issue_id = os.environ.get("WEAVER_ISSUE_ID", "")
        cmd = [weaver_bin]
        if db_path:
            cmd.extend(["--db", db_path])
        cmd.extend(["issue", "review-request", issue_id[:8]])
        summary = step.get("summary", "Review requested by mock agent")
        cmd.extend(["--summary", summary])
        subprocess.run(cmd, check=True, capture_output=True)

    elif action == "result":
        emit_result(
            result=step.get("result", "done"),
            model=step.get("model", "mock-model"),
            input_tokens=step.get("input_tokens", 100),
            output_tokens=step.get("output_tokens", 50),
            cost_usd=step.get("cost_usd", 0.01),
        )

    elif action == "fail":
        sys.exit(1)


def load_program() -> list[dict]:
    programs_dir = os.environ.get("MOCK_PROGRAMS_DIR", "")
    issue_id = os.environ.get("WEAVER_ISSUE_ID", "")

    # Try per-issue program
    if programs_dir and issue_id:
        path = os.path.join(programs_dir, f"{issue_id}.json")
        if os.path.exists(path):
            with open(path) as f:
                return json.load(f)

    # Try default program
    if programs_dir:
        path = os.path.join(programs_dir, "_default.json")
        if os.path.exists(path):
            with open(path) as f:
                return json.load(f)

    # Built-in fallback
    return [
        {"action": "init"},
        {"action": "text", "text": "Mock agent completed."},
        {"action": "result", "result": "done"},
    ]


def main() -> None:
    steps = load_program()
    if not steps:
        return

    # Auto-emit init if first step isn't init
    if steps[0].get("action") != "init":
        emit_init()

    has_result = any(s.get("action") in ("result", "fail") for s in steps)

    for step in steps:
        execute_step(step)

    # Auto-emit result if no result/fail action
    if not has_result:
        emit_result()


if __name__ == "__main__":
    main()
