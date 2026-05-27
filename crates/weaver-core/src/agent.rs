//! Agent-facing helpers that are pure (no tmux, no process spawning): the
//! Claude Code hook config and the SessionStart primer.

use serde_json::{json, Value};

/// Claude Code hook config that reports session status to weaver.
///
/// Hooks shell out to `weaver hook --event <name>`. The CLI itself resolves the
/// current branch (from `$WEAVER_BRANCH` or cwd) and writes an `events` row;
/// the orchestrator picks it up on its monitor tick. No daemon required.
pub fn hooks_json(weaver_bin: &str) -> Value {
    let hook = |event: &str| {
        json!([{
            "hooks": [{
                "type": "command",
                "command": format!("{weaver_bin} hook --event {event}")
            }]
        }])
    };
    json!({
        "hooks": {
            "SessionStart": hook("session-start"),
            "UserPromptSubmit": hook("working"),
            "Notification": hook("waiting"),
            "Stop": hook("idle"),
        }
    })
}

/// The session primer, kept as a standalone markdown doc and catted in at
/// build time so `weaver hook` stays self-contained wherever it runs.
const PRIMER: &str = include_str!("../primer.md");

/// Context injected at SessionStart (via the `session-start` weaver hook): tells
/// the agent it is in a weaver session and how it is expected to behave.
pub fn session_primer() -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": PRIMER,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_point_at_the_weaver_binary() {
        let hooks = hooks_json("/usr/bin/weaver");
        let stop = hooks["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(stop, "/usr/bin/weaver hook --event idle");
    }

    #[test]
    fn session_start_hook_uses_a_distinct_event() {
        let hooks = hooks_json("/usr/bin/weaver");
        let cmd = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(cmd, "/usr/bin/weaver hook --event session-start");
    }

    #[test]
    fn session_primer_is_session_start_additional_context() {
        let v: Value = serde_json::from_str(&session_primer()).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert!(v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("weaver note"));
    }
}
