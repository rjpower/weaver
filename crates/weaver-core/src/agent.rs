//! Agent-facing helpers that are pure (no terminal management, no process spawning): the
//! Claude Code hook config and the SessionStart primer (a WEAVER.md).

use serde_json::{json, Map, Value};

/// Which hook bundle [`hooks_json`] installs, chosen by the session's execution
/// backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookMode {
    /// The full lifecycle bundle for a terminal (PTY) session: `SessionStart`
    /// (primer) plus the work-cycle hooks (`UserPromptSubmit`/`Notification`/
    /// `Stop`) that drive working/waiting/idle.
    Terminal,
    /// Only `SessionStart` (primer + compaction re-orientation) — for an ACP
    /// session, whose turn boundaries come from the protocol itself, so the
    /// work-cycle hooks are redundant and are dropped (loom drives status/idle
    /// from the ACP turn edges instead; see `loom::acp` / `loom::monitor`).
    Acp,
}

/// Claude Code hook config that reports session status to weaver.
///
/// Hooks shell out to `weaver hook --event <name>`. The CLI itself resolves the
/// current branch (from `$WEAVER_BRANCH` or cwd) and writes an `events` row;
/// the orchestrator picks it up on its monitor tick. No daemon required.
///
/// `mode` selects the bundle: a terminal session installs the full set;
/// an ACP session installs only `SessionStart`, since its working/idle edges
/// are the protocol's turn boundaries, not the work-cycle hooks.
pub fn hooks_json(weaver_bin: &str, mode: HookMode) -> Value {
    let hook = |event: &str| {
        json!([{
            "hooks": [{
                "type": "command",
                "command": format!("{weaver_bin} hook --event {event}")
            }]
        }])
    };
    let mut hooks = Map::new();
    hooks.insert("SessionStart".into(), hook("session-start"));
    if mode == HookMode::Terminal {
        hooks.insert("UserPromptSubmit".into(), hook("working"));
        hooks.insert("Notification".into(), hook("waiting"));
        hooks.insert("Stop".into(), hook("idle"));
    }
    json!({ "hooks": hooks })
}

/// The builtin WEAVER.md — how an agent works inside a weaver session — kept as
/// a standalone markdown doc and catted in at build time so `weaver hook` stays
/// self-contained wherever it runs. A repo may ship its own `WEAVER.md` to
/// override this; see [`session_primer`].
const BUILTIN_WEAVER_MD: &str = include_str!("../WEAVER.md");

/// The builtin WEAVER.md, used when the repo doesn't ship its own.
pub fn builtin_weaver_md() -> &'static str {
    BUILTIN_WEAVER_MD
}

/// Wrap `context` as the JSON a SessionStart hook prints to inject it into the
/// agent's context (`hookSpecificOutput.additionalContext`). On a genuine
/// start/resume/clear this carries the full WEAVER.md primer (the repo's own
/// when present, else [`builtin_weaver_md`]); after a compaction the weaver hook
/// passes a concise re-orientation instead, so the agent isn't re-fed the whole
/// guide every time its context is summarized.
pub fn session_primer(context: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": context,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_point_at_the_weaver_binary() {
        let hooks = hooks_json("/usr/bin/weaver", HookMode::Terminal);
        let stop = hooks["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(stop, "/usr/bin/weaver hook --event idle");
    }

    #[test]
    fn session_start_hook_uses_a_distinct_event() {
        let hooks = hooks_json("/usr/bin/weaver", HookMode::Terminal);
        let cmd = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(cmd, "/usr/bin/weaver hook --event session-start");
    }

    #[test]
    fn acp_mode_installs_only_the_session_start_hook() {
        // An ACP session's turn boundaries are the protocol's, so only the
        // primer hook is installed — the work-cycle hooks are dropped.
        let hooks = hooks_json("/usr/bin/weaver", HookMode::Acp);
        let obj = hooks["hooks"].as_object().unwrap();
        assert_eq!(
            obj.keys().collect::<Vec<_>>(),
            vec!["SessionStart"],
            "only SessionStart is installed for acp: {hooks}"
        );
        assert!(obj.get("UserPromptSubmit").is_none());
        assert!(obj.get("Stop").is_none());
        assert!(obj.get("Notification").is_none());
    }

    #[test]
    fn session_primer_wraps_the_builtin_weaver_md() {
        let v: Value = serde_json::from_str(&session_primer(builtin_weaver_md())).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert!(v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("weaver status"));
    }

    #[test]
    fn session_primer_passes_a_repo_override_through_verbatim() {
        let custom = "# Our team's weaver workflow\nrun `make ci` before any PR.";
        let v: Value = serde_json::from_str(&session_primer(custom)).unwrap();
        assert_eq!(v["hookSpecificOutput"]["additionalContext"], custom);
    }
}
