//! Launching coding agents into per-session terminals and installing status hooks, plus
//! the **one-shot headless agent** (`POST /api/agent/oneshot`) — a fresh,
//! env-stripped `claude -p` run for a judgement call.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::backend;
use weaver_core::agent::hooks_json;

/// Accepted per-session reasoning effort levels, increasing. Orthogonal to the
/// model: any model can run at any effort.
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Accepted per-session model tiers.
pub const MODELS: &[&str] = &["haiku", "sonnet", "opus", "fable"];

/// `--effort <level>` for a known level, else empty (inherit the configured
/// `agent.claude_args`).
pub fn effort_args(effort: &str) -> String {
    match effort.trim() {
        "" => String::new(),
        e => format!("--effort {e}"),
    }
}

/// `--model <tier>` for a chosen model, else empty.
pub fn model_args(model: &str) -> String {
    match model.trim() {
        "" => String::new(),
        m => format!("--model {m}"),
    }
}

/// Combine the configured base `agent.claude_args` with the per-session model
/// and effort selections. Each non-empty part is appended in turn, so a session
/// layers its model/effort on top of the global flags.
pub fn combine_args(base: &str, model: &str, effort: &str) -> String {
    [base.trim(), &model_args(model), &effort_args(effort)]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a session's terminal is being created for the first time or
/// recreated to recover ("adopt") an existing worktree whose session died.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    /// First launch: seed the agent with the branch's goal.
    Fresh,
    /// Re-launch into an existing worktree: resume rather than restart.
    Adopt,
}

fn inner_command(
    agent_kind: &str,
    goal_file: Option<&Path>,
    mode: LaunchMode,
    claude_args: &str,
) -> String {
    let args = match claude_args.trim() {
        "" => String::new(),
        a => format!(" {a}"),
    };
    match agent_kind {
        "shell" | "none" => String::new(),
        "claude" => match mode {
            LaunchMode::Adopt => format!("claude --continue{args}"),
            LaunchMode::Fresh => match goal_file {
                Some(f) => format!("claude{args} \"$(cat '{}')\"", f.display()),
                None => format!("claude{args}"),
            },
        },
        other => match goal_file {
            Some(f) => format!("{other} '{}'", f.display()),
            None => other.to_string(),
        },
    }
}

pub fn launch_script(
    agent_kind: &str,
    goal_file: Option<&Path>,
    env: &[(&str, &str)],
    weaver_dir: Option<&Path>,
    mode: LaunchMode,
    claude_args: &str,
) -> String {
    let mut script = String::new();
    if let Some(dir) = weaver_dir {
        script.push_str(&format!("export PATH=\"{}:$PATH\"; ", dir.display()));
    }
    for (k, v) in env {
        script.push_str(&format!("export {k}='{v}'; "));
    }
    let inner = inner_command(agent_kind, goal_file, mode, claude_args);
    if !inner.is_empty() {
        script.push_str(&inner);
        script.push_str("; ");
    }
    script.push_str("exec \"${SHELL:-/bin/sh}\"");
    script
}

/// Everything [`launch`] needs to bring up a session's terminal.
pub struct LaunchSpec<'a> {
    /// The branch id — the agent uses this to resolve "its" branch via
    /// `$WEAVER_BRANCH`.
    pub branch_id: &'a str,
    pub agent_kind: &'a str,
    pub work_dir: &'a Path,
    pub term_session: &'a str,
    pub goal_file: Option<&'a Path>,
    pub server_addr: &'a str,
    pub claude_args: &'a str,
}

/// Bring up the session's terminal running the agent.
pub async fn launch(spec: &LaunchSpec<'_>, mode: LaunchMode) -> Result<()> {
    let loom_exe = std::env::current_exe().ok();
    let weaver_dir = loom_exe.as_deref().and_then(Path::parent);
    let weaver_bin = weaver_dir
        .map(|d| d.join("weaver").display().to_string())
        .unwrap_or_else(|| "weaver".to_string());

    if spec.agent_kind == "claude" {
        install_hooks(spec.work_dir, &weaver_bin).await.ok();
        // A fresh container HOME hasn't cleared Claude Code's first-run gates
        // (theme picker, workspace trust, API-key + bypass-mode prompts), so an
        // unattended `claude` would stall — or quit — before reading the goal.
        // Pre-seed that state so it runs. Pass the resolved args so the
        // bypass-mode gate is only seeded when we actually launch in that mode.
        seed_claude_launch_gates(spec.work_dir, spec.claude_args)
            .await
            .ok();
    }

    let api_url = format!("http://{}", spec.server_addr);
    // Hand the agent the machine-local token so its in-worktree `loom session …`
    // calls authenticate even when loopback trust is off. Absent file ⇒ omit it
    // (loopback trust then covers the local case).
    let local_token = read_local_token();
    let mut env = vec![
        ("WEAVER_API", api_url.as_str()),
        ("WEAVER_BRANCH", spec.branch_id),
    ];
    if let Some(token) = local_token.as_deref() {
        env.push(("LOOM_TOKEN", token));
    }
    let script = launch_script(
        spec.agent_kind,
        spec.goal_file,
        &env,
        weaver_dir,
        mode,
        spec.claude_args,
    );
    tracing::debug!(
        branch = spec.branch_id,
        agent_kind = spec.agent_kind,
        session = spec.term_session,
        ?mode,
        "launching agent session"
    );
    backend::new_session(spec.term_session, spec.work_dir, &script)
        .await
        .with_context(|| format!("terminal: launching session {}", spec.term_session))?;
    tracing::info!(
        branch = spec.branch_id,
        agent_kind = spec.agent_kind,
        session = spec.term_session,
        ?mode,
        "agent launched"
    );
    Ok(())
}

/// The machine-local bearer token (trimmed), if the daemon has minted it. Read
/// straight off disk so callers needn't thread it through; absent ⇒ `None`.
pub fn read_local_token() -> Option<String> {
    std::fs::read_to_string(crate::auth::local_token_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Write (merging into any existing file) `.claude/settings.local.json` so the
/// agent reports status to weaver via hooks.
pub async fn install_hooks(work_dir: &Path, weaver_bin: &str) -> Result<()> {
    let dir = work_dir.join(".claude");
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join("settings.local.json");
    let mut root: Value = match tokio::fs::read_to_string(&path).await {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    };
    let hooks = hooks_json(weaver_bin);
    root["hooks"] = hooks["hooks"].clone();
    tokio::fs::write(&path, serde_json::to_string_pretty(&root)?).await?;
    tracing::debug!(path = %path.display(), "claude hooks installed");
    Ok(())
}

/// Pre-clear Claude Code's first-run interactive gates in the agent user's
/// global `~/.claude.json` so a detached, unattended `claude` runs its task
/// instead of stalling — or *quitting* — at a prompt no human can answer.
///
/// On a fresh, persisted container HOME these gates fire in sequence and each
/// wedges the session (it sits at "launching" with no `weaver status`, worktree
/// idle). Each gate is just state Claude records after a human answers once; we
/// write the same state ahead of time. Everything here is additive and
/// idempotent — only missing/false gates are set, existing config is preserved —
/// so it is safe to re-run before every launch. Gates handled:
///
/// * `hasCompletedOnboarding` + `theme` — the first-run theme picker.
/// * `projects.<repo-root>.hasTrustDialogAccepted` — the workspace-trust dialog.
///   Claude resolves a git worktree back to its **main repo root** and records
///   trust there, so trusting the root once covers every worktree under it.
/// * `customApiKeyResponses.approved` — the "use this `ANTHROPIC_API_KEY`?"
///   prompt, keyed by the key's last 20 chars; seeded only when that env var is
///   set (i.e. the agent authenticates by API key).
/// * `bypassPermissionsModeAccepted` — the one-time "you're in Bypass
///   Permissions mode" confirmation, **which defaults to *exit***. Seeded only
///   when this launch actually runs with a bypass flag, since the dialog only
///   appears then.
pub async fn seed_claude_launch_gates(work_dir: &Path, claude_args: &str) -> Result<()> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        tracing::debug!("HOME unset; skipping claude launch-gate seed");
        return Ok(());
    };
    let path = home.join(".claude.json");
    let mut root: Value = match tokio::fs::read_to_string(&path).await {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    };
    if !root.is_object() {
        root = json!({});
    }
    let obj = root.as_object_mut().expect("root is an object");
    let mut changed = false;

    // 1. Onboarding / theme picker.
    if obj.get("hasCompletedOnboarding").and_then(Value::as_bool) != Some(true) {
        obj.insert("hasCompletedOnboarding".into(), json!(true));
        changed = true;
    }
    if !obj.contains_key("theme") {
        obj.insert("theme".into(), json!("dark"));
        changed = true;
    }

    // 2. Bypass-permissions acceptance — only when we launch in that mode.
    let bypass = claude_args.contains("--dangerously-skip-permissions")
        || claude_args.contains("bypassPermissions");
    if bypass && obj.get("bypassPermissionsModeAccepted").and_then(Value::as_bool) != Some(true) {
        obj.insert("bypassPermissionsModeAccepted".into(), json!(true));
        changed = true;
    }

    // 3. Ambient ANTHROPIC_API_KEY approval (keyed by the key's last 20 chars).
    if let Some(key) = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| k.len() >= 20) {
        let tail = key[key.len() - 20..].to_string();
        let entry = obj
            .entry("customApiKeyResponses")
            .or_insert_with(|| json!({"approved": [], "rejected": []}));
        if !entry.is_object() {
            *entry = json!({"approved": [], "rejected": []});
        }
        let entry = entry.as_object_mut().unwrap();
        if !entry.get("approved").map(Value::is_array).unwrap_or(false) {
            entry.insert("approved".into(), json!([]));
        }
        let approved = entry.get_mut("approved").unwrap().as_array_mut().unwrap();
        if !approved.iter().any(|v| v.as_str() == Some(tail.as_str())) {
            approved.push(json!(tail));
            changed = true;
        }
        if !entry.contains_key("rejected") {
            entry.insert("rejected".into(), json!([]));
        }
    }

    // 4. Workspace trust, recorded at the worktree's main repo root.
    match weaver_core::git::repo_root(work_dir).await {
        Ok(repo_root) => {
            let key = repo_root.to_string_lossy().to_string();
            let projects = obj.entry("projects").or_insert_with(|| json!({}));
            if !projects.is_object() {
                *projects = json!({});
            }
            let proj = projects
                .as_object_mut()
                .unwrap()
                .entry(key)
                .or_insert_with(|| json!({}));
            if !proj.is_object() {
                *proj = json!({});
            }
            let proj = proj.as_object_mut().unwrap();
            if proj.get("hasTrustDialogAccepted").and_then(Value::as_bool) != Some(true) {
                proj.insert("hasTrustDialogAccepted".into(), json!(true));
                changed = true;
            }
        }
        Err(e) => tracing::debug!(work_dir = %work_dir.display(), error = %e,
            "could not resolve repo root for trust seed"),
    }

    if !changed {
        return Ok(());
    }
    tokio::fs::write(&path, serde_json::to_string_pretty(&root)?)
        .await
        .with_context(|| format!("seeding {}", path.display()))?;
    // claude writes this file 0600; preserve that posture.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await;
    }
    tracing::info!(path = %path.display(), bypass, "seeded claude launch gates");
    Ok(())
}

// ---------------------------------------------------------------------------
// The one-shot headless agent
// ---------------------------------------------------------------------------

/// Markers of a *calling* Claude Code session, stripped before spawning a
/// subprocess so it runs fresh and isolated (the lint-review precedent).
/// Mirrors `scripts/lint-review.py`'s `STRIPPED_ENV`. Shared by the one-shot
/// agent here and the overlooker script executor.
pub const STRIPPED_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SSE_PORT",
];

/// Spawn a one-shot headless agent: write `prompt` to its stdin, capture
/// stdout, strip the calling session's env markers. Best-effort: returns
/// `None` when the agent is absent, errors, or exceeds `timeout` — callers
/// must degrade gracefully, so a missing `claude` never breaks them.
///
/// The command is `WEAVER_OVERLOOKER_AGENT_CMD` (default `claude -p`); a
/// non-empty `model`/`effort` is appended as `--model`/`--effort`.
pub async fn run_oneshot(
    prompt: &str,
    model: &str,
    effort: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    let cmd_str =
        std::env::var("WEAVER_OVERLOOKER_AGENT_CMD").unwrap_or_else(|_| "claude -p".to_string());
    let mut parts = cmd_str.split_whitespace();
    let program = parts.next()?;
    let mut args: Vec<String> = parts.map(str::to_string).collect();
    if !model.trim().is_empty() {
        args.push("--model".to_string());
        args.push(model.trim().to_string());
    }
    if !effort.trim().is_empty() {
        args.push("--effort".to_string());
        args.push(effort.trim().to_string());
    }

    let mut command = tokio::process::Command::new(program);
    command
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    for key in STRIPPED_ENV {
        command.env_remove(key);
    }

    let mut child = command.spawn().ok()?; // agent not on PATH → None, caller degrades.
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(prompt.as_bytes()).await;
        // Drop stdin so the agent sees EOF and proceeds.
        drop(stdin);
    }

    let out = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match out {
        Ok(Ok(output)) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn shell_script_just_execs_a_shell() {
        let script = launch_script("shell", None, &[], None, LaunchMode::Fresh, "");
        assert_eq!(script, "exec \"${SHELL:-/bin/sh}\"");
    }

    #[test]
    fn claude_script_exports_env_and_runs_claude() {
        let script = launch_script(
            "claude",
            Some(Path::new("/x/goal.txt")),
            &[("WEAVER_API", "http://h:1")],
            None,
            LaunchMode::Fresh,
            "",
        );
        assert!(script.contains("export WEAVER_API='http://h:1'; "));
        assert!(script.contains("claude \"$(cat '/x/goal.txt')\"; "));
        assert!(script.ends_with("exec \"${SHELL:-/bin/sh}\""));
    }

    #[test]
    fn effort_and_model_args() {
        assert_eq!(effort_args("xhigh"), "--effort xhigh");
        assert_eq!(effort_args(""), "");
        assert_eq!(model_args("opus"), "--model opus");
        assert_eq!(model_args("fable"), "--model fable");
        assert_eq!(model_args(""), "");
    }

    #[test]
    fn combine_args_layers_model_and_effort_onto_base() {
        assert_eq!(
            combine_args("", "opus", "high"),
            "--model opus --effort high"
        );
        assert_eq!(combine_args("--verbose", "", ""), "--verbose");
        assert_eq!(combine_args("", "", "max"), "--effort max");
        assert_eq!(combine_args("", "haiku", ""), "--model haiku");
        assert_eq!(
            combine_args("--verbose", "sonnet", "low"),
            "--verbose --model sonnet --effort low"
        );
        assert_eq!(combine_args("", "", ""), "");
    }

    #[test]
    fn adopt_mode_resumes_claude_with_continue() {
        let script = launch_script(
            "claude",
            Some(Path::new("/x/goal.txt")),
            &[],
            None,
            LaunchMode::Adopt,
            "",
        );
        assert_eq!(script, "claude --continue; exec \"${SHELL:-/bin/sh}\"");
    }
}
