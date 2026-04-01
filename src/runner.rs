use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::issue::Issue;
use crate::sandbox::{generate_profile, SandboxLevel};

pub struct AgentResult {
    pub result: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub model: Option<String>,
    pub session_id: Option<String>,
}

/// Events emitted by the agent process during execution.
/// Stored in the `issue_events` table and streamed to the frontend via SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    Init {
        session_id: String,
        model: String,
    },
    Text {
        text: String,
    },
    ToolUse {
        tool: String,
        input: String,
    },
    ToolResult {
        tool: String,
        output: String,
    },
    Result {
        result: String,
        input_tokens: i64,
        output_tokens: i64,
        model: Option<String>,
        cost_usd: f64,
    },
    Error {
        message: String,
    },
}

/// Parse a single NDJSON line from `claude --print --output-format stream-json`
/// into zero or more `StreamEvent`s.
fn parse_stream_line(line: &str) -> Vec<StreamEvent> {
    let json: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let msg_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match msg_type {
        "system" => {
            let session_id = json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let model = json
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            vec![StreamEvent::Init { session_id, model }]
        }
        "assistant" => {
            let mut events = Vec::new();
            if let Some(blocks) = json.pointer("/message/content").and_then(|v| v.as_array()) {
                for block in blocks {
                    let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match block_type {
                        "text" => {
                            let text = block
                                .get("text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if !text.is_empty() {
                                events.push(StreamEvent::Text { text });
                            }
                        }
                        "tool_use" => {
                            let tool = block
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let input = block
                                .get("input")
                                .map(|v| v.to_string())
                                .unwrap_or_default();
                            events.push(StreamEvent::ToolUse {
                                tool,
                                input,
                            });
                        }
                        "tool_result" => {
                            let tool = block
                                .get("tool_use_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let output = block
                                .get("content")
                                .map(|v| {
                                    if let Some(s) = v.as_str() {
                                        s.to_string()
                                    } else {
                                        v.to_string()
                                    }
                                })
                                .unwrap_or_default();
                            events.push(StreamEvent::ToolResult {
                                tool,
                                output,
                            });
                        }
                        _ => {}
                    }
                }
            }
            events
        }
        "result" => {
            let result = json
                .get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let input_tokens = json
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let output_tokens = json
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let model = json.get("model").and_then(|v| v.as_str()).map(String::from);
            let cost_usd = json
                .get("total_cost_usd")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            vec![StreamEvent::Result {
                result,
                input_tokens,
                output_tokens,
                model,
                cost_usd,
            }]
        }
        _ => vec![],
    }
}

/// Configure a subprocess so it cannot steal the TTY and is cleaned up on drop.
///
/// - `stdin(null)` prevents the child from reading the terminal
/// - `process_group(0)` on Unix puts the child in its own process group so
///   Ctrl-C (SIGINT) is delivered only to weaver, not to children
/// - `kill_on_drop(true)` ensures tokio sends SIGKILL if the Child handle is
///   dropped (e.g. on timeout or cancellation)
fn configure_subprocess(cmd: &mut Command) {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
}

pub struct AgentRunner {
    pub api_url: String,
    /// Directory containing built-in skill templates (.md files).
    /// Named `workflows_dir` for backward compat with weaver binary (updated separately).
    pub workflows_dir: PathBuf,
    pub binary: String,
    // Kept for backward compat until weaver binary is updated in a separate task.
    #[allow(dead_code)]
    pub sdk_dir: PathBuf,
}

impl AgentRunner {
    pub fn skills_dir(&self) -> &Path {
        &self.workflows_dir
    }
}

/// Walk up from `start` looking for a directory containing `weaver/skills/builtins/`.
/// Returns the `weaver/skills/builtins` path if found.
fn walk_up_for_skills(start: &Path, stop_at_git_root: bool) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let candidate = d.join("weaver/skills/builtins");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if stop_at_git_root && d.join(".git").exists() {
            return None;
        }
        dir = d.parent();
    }
    None
}

/// Discover the `weaver/skills/builtins` directory by searching:
/// 1. Up from the weaver binary location (handles cargo builds, installed binaries)
/// 2. Up from cwd to the git root (handles running from within the weaver repo)
pub fn find_skills_root() -> anyhow::Result<PathBuf> {
    if let Ok(exe) = std::env::current_exe().and_then(|p| p.canonicalize()) {
        if let Some(parent) = exe.parent() {
            if let Some(root) = walk_up_for_skills(parent, false) {
                return Ok(root);
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = walk_up_for_skills(&cwd, true) {
            return Ok(root);
        }
    }

    anyhow::bail!(
        "could not find weaver/skills/builtins/ directory (searched from binary location and cwd)"
    )
}

/// Parsed skill template with optional YAML frontmatter.
pub struct SkillTemplate {
    pub name: String,
    pub description: String,
    pub sandbox: Option<String>,
    pub body: String,
}

/// Parse a markdown file with optional YAML frontmatter (--- delimited).
/// If no frontmatter is present, the entire content becomes the body.
pub fn parse_skill_template(content: &str) -> anyhow::Result<SkillTemplate> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Ok(SkillTemplate {
            name: String::new(),
            description: String::new(),
            sandbox: None,
            body: content.to_string(),
        });
    }

    // Skip the opening "---" line
    let after_opening = match trimmed.strip_prefix("---") {
        Some(rest) => rest.trim_start_matches(['\r', '\n']),
        None => unreachable!(),
    };

    let Some(end_idx) = after_opening.find("\n---") else {
        // No closing delimiter — treat entire content as body
        return Ok(SkillTemplate {
            name: String::new(),
            description: String::new(),
            sandbox: None,
            body: content.to_string(),
        });
    };

    let yaml_str = &after_opening[..end_idx];
    let body_start = end_idx + 4; // skip "\n---"
    let body = after_opening[body_start..]
        .trim_start_matches(['\r', '\n'])
        .to_string();

    // Parse name and description from the YAML frontmatter.
    // Simple line-by-line parsing to avoid a full YAML dependency.
    let mut name = String::new();
    let mut description = String::new();
    let mut sandbox = None;
    for line in yaml_str.lines() {
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = line.strip_prefix("sandbox:") {
            sandbox = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }

    Ok(SkillTemplate {
        name,
        description,
        sandbox,
        body,
    })
}

/// Resolve a partial (include) name to a file path using the same layered
/// lookup as skill resolution: repo-local `.weaver/skills/{name}.md` first,
/// then builtin `skills_dir/{name}.md`.
pub fn resolve_partial(name: &str, skills_dir: &Path, work_dir: &Path) -> Option<PathBuf> {
    let local = work_dir.join(format!(".weaver/skills/{name}.md"));
    if local.exists() {
        return Some(local);
    }
    let builtin = skills_dir.join(format!("{name}.md"));
    if builtin.exists() {
        return Some(builtin);
    }
    None
}

/// Replace `{{var}}` placeholders with issue fields. This handles all
/// variable types: `issue_id`, `issue_id_short`, `issue_title`, `issue_body`,
/// `work_dir`, and `context.KEY`.
fn expand_variables(template: &str, issue: &Issue) -> String {
    let mut result = template.to_string();
    result = result.replace("{{issue_id}}", &issue.id);
    result = result.replace("{{issue_id_short}}", &issue.id);
    result = result.replace("{{issue_title}}", &issue.title);
    result = result.replace("{{issue_body}}", &issue.body);

    let work_dir = issue
        .context
        .get("work_dir")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    result = result.replace("{{work_dir}}", work_dir);

    // Expand {{context.KEY}} by looking up issue.context[KEY]
    while let Some(start) = result.find("{{context.") {
        let after = &result[start + 10..]; // skip "{{context."
        let Some(end) = after.find("}}") else {
            break;
        };
        let key = &after[..end];
        let value = match issue.context.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        };
        let placeholder = format!("{{{{context.{key}}}}}");
        result = result.replacen(&placeholder, &value, 1);
    }

    result
}

/// Recursively resolve `{{include:name}}` directives in template content.
/// Each included file is looked up via `resolve_partial`, has its frontmatter
/// stripped, and is then itself expanded for further includes.
fn expand_includes(
    content: &str,
    skills_dir: &Path,
    work_dir: &Path,
    visited: &mut HashSet<PathBuf>,
) -> anyhow::Result<String> {
    let mut result = content.to_string();

    while let Some(start) = result.find("{{include:") {
        let after = &result[start + 10..]; // skip "{{include:"
        let Some(end) = after.find("}}") else {
            break;
        };
        let name = &after[..end];
        let path = resolve_partial(name, skills_dir, work_dir)
            .ok_or_else(|| anyhow::anyhow!("include not found: {name}"))?;

        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !visited.insert(canonical.clone()) {
            anyhow::bail!("circular include detected: {name}");
        }

        let raw = std::fs::read_to_string(&path)?;
        let parsed = parse_skill_template(&raw)?;
        let expanded = expand_includes(&parsed.body, skills_dir, work_dir, visited)?;

        let placeholder = format!("{{{{include:{name}}}}}");
        result = result.replacen(&placeholder, &expanded, 1);
    }

    Ok(result)
}

/// Expand a skill template: first resolve all `{{include:name}}` directives
/// recursively, then substitute `{{var}}` placeholders with issue fields.
pub fn expand_template(
    template: &str,
    issue: &Issue,
    skills_dir: &Path,
    work_dir: &Path,
) -> anyhow::Result<String> {
    let expanded = expand_includes(template, skills_dir, work_dir, &mut HashSet::new())?;
    Ok(expand_variables(&expanded, issue))
}

impl AgentRunner {
    /// Resolve a tag to a skill template path.
    /// 1. Check .weaver/skills/{tag}.md (repo-local)
    /// 2. Check skills_dir/{tag}.md (built-in)
    /// Returns None if no template found.
    pub fn resolve_skill(&self, tag: &str, work_dir: &Path) -> Option<PathBuf> {
        let local = work_dir
            .join(".weaver/skills")
            .join(format!("{tag}.md"));
        if local.exists() {
            return Some(local);
        }
        let builtin = self.skills_dir().join(format!("{tag}.md"));
        if builtin.exists() {
            return Some(builtin);
        }
        None
    }

    /// Spawn `claude --print --output-format stream-json` with a prompt.
    /// Reads stdout line-by-line as NDJSON, parsing each line into `StreamEvent`s.
    /// If `event_tx` is provided, events are sent for external consumption
    /// (e.g., storage in DB for SSE streaming to the frontend).
    ///
    /// Sets `WEAVER_API_URL` and `WEAVER_ISSUE_ID` env vars on the subprocess
    /// so agents can interact with the Weaver API.
    pub async fn call_agent(
        &self,
        prompt: &str,
        work_dir: &Path,
        context: Value,
        skill_template: Option<&str>,
        issue_id: &str,
        event_tx: Option<mpsc::Sender<StreamEvent>>,
    ) -> anyhow::Result<AgentResult> {
        const MAX_API_RETRIES: u32 = 3;

        let sandbox_level = context
            .get("sandbox")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<SandboxLevel>())
            .transpose()?
            .unwrap_or(SandboxLevel::Unrestricted);

        let use_sandbox =
            cfg!(target_os = "macos") && sandbox_level != SandboxLevel::Unrestricted;

        let sandbox_profile = if use_sandbox {
            Some(generate_profile(sandbox_level, work_dir))
        } else {
            if !cfg!(target_os = "macos") && sandbox_level != SandboxLevel::Unrestricted {
                tracing::warn!(sandbox = %sandbox_level, "sandbox-exec not available on this platform, running unsandboxed");
            }
            None
        };

        // Build args list once, reuse for retries
        let is_resume = context.get("resume_id").and_then(|v| v.as_str()).is_some();
        let mut args = vec![
            "--print".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
            "--dangerously-skip-permissions".to_string(),
        ];
        if let Some(model) = context.get("model").and_then(|v| v.as_str()) {
            args.push("--model".to_string());
            args.push(model.to_string());
        }
        if !is_resume {
            let mut system_parts = Vec::new();
            if let Some(skill_body) = skill_template {
                system_parts.push(skill_body.to_string());
            }
            if let Some(user_prompt) = context.get("system_prompt").and_then(|v| v.as_str()) {
                system_parts.push(user_prompt.to_string());
            }
            let full_system_prompt = system_parts.join("\n\n");
            args.push("--system-prompt".to_string());
            args.push(full_system_prompt);
        }
        if let Some(resume_id) = context.get("resume_id").and_then(|v| v.as_str()) {
            args.push("--resume".to_string());
            args.push(resume_id.to_string());
        }
        args.push(prompt.to_string());

        let mut last_error = None;

        for attempt in 0..=MAX_API_RETRIES {
            if attempt > 0 {
                let delay = Duration::from_secs(2u64.pow(attempt - 1));
                tracing::info!(issue_id, attempt, delay_secs = delay.as_secs(), "Retrying after API error");
                tokio::time::sleep(delay).await;
            }

            let mut cmd = if let Some(ref profile) = sandbox_profile {
                let mut c = Command::new("sandbox-exec");
                c.arg("-p").arg(profile).arg("--").arg(&self.binary);
                c
            } else {
                Command::new(&self.binary)
            };
            cmd.env_remove("CLAUDECODE")
                .env_remove("CLAUDE_CODE_ENTRYPOINT")
                .env_remove("CLAUDE_CODE_SSE_PORT")
                .env_remove("ANTHROPIC_API_KEY");
            cmd.env("WEAVER_API_URL", &self.api_url)
                .env("WEAVER_ISSUE_ID", issue_id)
                .env("WEAVER_WORK_DIR", work_dir);
            cmd.args(&args);
            cmd.current_dir(work_dir);
            configure_subprocess(&mut cmd);

            let mut child = cmd.spawn()?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("failed to capture agent stdout"))?;
            let stderr = child.stderr.take();

            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            let mut final_result: Option<AgentResult> = None;
            let mut captured_session_id: Option<String> = None;

            'read: while let Some(line) = lines.next_line().await? {
                let events = parse_stream_line(&line);
                for event in events {
                    if let StreamEvent::Init { ref session_id, .. } = event {
                        captured_session_id = Some(session_id.clone());
                    }
                    if let StreamEvent::Result {
                        ref result,
                        input_tokens,
                        output_tokens,
                        ref model,
                        ..
                    } = event
                    {
                        final_result = Some(AgentResult {
                            result: result.clone(),
                            input_tokens,
                            output_tokens,
                            model: model.clone(),
                            session_id: captured_session_id.clone(),
                        });
                        if let Some(ref tx) = event_tx {
                            tx.send(event).await.ok();
                        }
                        // Stop reading — any subsequent output is from background
                        // task resumptions and should not affect this result.
                        break 'read;
                    }
                    if let Some(ref tx) = event_tx {
                        tx.send(event).await.ok();
                    }
                }
            }

            let status = child.wait().await?;

            if let Some(ref fr) = final_result {
                if is_retryable_api_error(&fr.result) && attempt < MAX_API_RETRIES {
                    tracing::warn!(issue_id, attempt = attempt + 1, "Result contains API error, will retry");
                    if let Some(ref tx) = event_tx {
                        tx.send(StreamEvent::Error {
                            message: format!("API error in result (attempt {}), retrying...", attempt + 1),
                        })
                        .await
                        .ok();
                    }
                    last_error = Some(format!("API error in result: {}", &fr.result[..fr.result.len().min(200)]));
                    final_result = None;
                    continue;
                }
                return Ok(final_result.unwrap());
            }

            if !status.success() {
                // Read stderr to check for retryable API errors
                let stderr_text = if let Some(se) = stderr {
                    let mut buf = String::new();
                    tokio::io::AsyncReadExt::read_to_string(
                        &mut tokio::io::BufReader::new(se),
                        &mut buf,
                    )
                    .await
                    .ok();
                    buf
                } else {
                    String::new()
                };

                let is_retryable = is_retryable_api_error(&stderr_text);
                let err_msg = if is_retryable {
                    format!("claude API error (attempt {}): {}", attempt + 1, status)
                } else {
                    format!("claude exited with {}", status)
                };

                if is_retryable && attempt < MAX_API_RETRIES {
                    tracing::warn!(issue_id, attempt = attempt + 1, "Retryable API error, will retry");
                    if let Some(ref tx) = event_tx {
                        tx.send(StreamEvent::Error {
                            message: format!("API error (attempt {}), retrying...", attempt + 1),
                        })
                        .await
                        .ok();
                    }
                    last_error = Some(err_msg);
                    continue;
                }

                anyhow::bail!("{err_msg}");
            }

            return Err(anyhow::anyhow!("no result message in agent stream output"));
        }

        Err(anyhow::anyhow!(
            "exhausted {} API retries: {}",
            MAX_API_RETRIES,
            last_error.unwrap_or_default()
        ))
    }
}

/// Check if stderr output indicates a retryable API error (5xx, overloaded).
fn is_retryable_api_error(stderr: &str) -> bool {
    // Match API 500/529 errors and overloaded messages
    stderr.contains("500") || stderr.contains("529") || stderr.contains("overloaded")
}

/// Check each tag against skill template files. Returns the first tag that
/// resolves to a .md template in either the repo-local or builtins directory.
pub fn find_skill_tag(tags: &[String], skills_dir: &Path, work_dir: &Path) -> Option<String> {
    for tag in tags {
        let local = work_dir
            .join(".weaver/skills")
            .join(format!("{tag}.md"));
        if local.exists() {
            return Some(tag.clone());
        }
        let builtin = skills_dir.join(format!("{tag}.md"));
        if builtin.exists() {
            return Some(tag.clone());
        }
    }
    None
}

#[derive(Debug)]
pub struct SkillInfo {
    pub tag: String,
    pub source: SkillSource,
    pub path: PathBuf,
}

#[derive(Debug, PartialEq)]
pub enum SkillSource {
    Builtin,
    Local,
    /// Local skill that shadows a builtin with the same tag.
    Override,
}

impl std::fmt::Display for SkillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Builtin => write!(f, "builtin"),
            Self::Local => write!(f, "local"),
            Self::Override => write!(f, "override"),
        }
    }
}

/// Collect all available skills from builtins and repo-local, noting overrides.
pub fn list_skills(builtins_dir: &Path, work_dir: &Path) -> Vec<SkillInfo> {
    let mut skills = Vec::new();
    let mut builtin_tags = std::collections::HashSet::new();

    if let Ok(entries) = std::fs::read_dir(builtins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem.starts_with('_') {
                        continue;
                    }
                    builtin_tags.insert(stem.to_string());
                    skills.push(SkillInfo {
                        tag: stem.to_string(),
                        source: SkillSource::Builtin,
                        path,
                    });
                }
            }
        }
    }

    let local_dir = work_dir.join(".weaver/skills");
    if let Ok(entries) = std::fs::read_dir(&local_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem.starts_with('_') {
                        continue;
                    }
                    let source = if builtin_tags.contains(stem) {
                        SkillSource::Override
                    } else {
                        SkillSource::Local
                    };
                    skills.push(SkillInfo {
                        tag: stem.to_string(),
                        source,
                        path,
                    });
                }
            }
        }
    }

    skills.sort_by(|a, b| a.tag.cmp(&b.tag));
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_template_with_frontmatter() {
        let content = "---\nname: design\ndescription: Create a design doc\n---\nYou are a designer.\n\nCreate a design for {{issue_title}}.";
        let template = parse_skill_template(content).unwrap();
        assert_eq!(template.name, "design");
        assert_eq!(template.description, "Create a design doc");
        assert!(template.body.starts_with("You are a designer."));
        assert!(template.body.contains("{{issue_title}}"));
    }

    #[test]
    fn parse_template_without_frontmatter() {
        let content = "Just a plain template body.";
        let template = parse_skill_template(content).unwrap();
        assert_eq!(template.name, "");
        assert_eq!(template.description, "");
        assert_eq!(template.body, "Just a plain template body.");
    }

    #[test]
    fn parse_template_with_unclosed_frontmatter() {
        let content = "---\nname: broken\nNo closing delimiter here.";
        let template = parse_skill_template(content).unwrap();
        // Falls back to treating entire content as body
        assert_eq!(template.body, content);
    }

    #[test]
    fn expand_template_replaces_variables() {
        let issue = Issue {
            id: "abcdefgh-1234-5678-9012-ijklmnopqrst".into(),
            title: "Fix the bug".into(),
            body: "There is a bug in main.rs".into(),
            status: crate::issue::IssueStatus::Pending,
            context: serde_json::json!({"work_dir": "/tmp/work", "repo": "my-repo"}),
            dependencies: vec![],
            num_tries: 0,
            max_tries: 3,
            parent_issue_id: None,
            tags: vec![],
            priority: 0,
            channel_kind: None,
            origin_ref: None,
            user_id: None,
            error: None,
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
            claude_session_id: None,
        };

        let template = "Issue: {{issue_id_short}} - {{issue_title}}\n\
                         Body: {{issue_body}}\n\
                         Dir: {{work_dir}}\n\
                         Repo: {{context.repo}}";
        let expanded =
            expand_template(template, &issue, Path::new("/nonexistent"), Path::new("/nonexistent"))
                .unwrap();
        assert!(expanded.contains("abcdefgh"));
        assert!(expanded.contains("Fix the bug"));
        assert!(expanded.contains("There is a bug in main.rs"));
        assert!(expanded.contains("/tmp/work"));
        assert!(expanded.contains("my-repo"));
    }

    #[test]
    fn expand_template_missing_context_key() {
        let issue = Issue {
            id: "abcdefgh-1234-5678-9012-ijklmnopqrst".into(),
            title: "Test".into(),
            body: String::new(),
            status: crate::issue::IssueStatus::Pending,
            context: serde_json::json!({}),
            dependencies: vec![],
            num_tries: 0,
            max_tries: 3,
            parent_issue_id: None,
            tags: vec![],
            priority: 0,
            channel_kind: None,
            origin_ref: None,
            user_id: None,
            error: None,
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
            claude_session_id: None,
        };

        let expanded = expand_template(
            "Key: {{context.missing}}",
            &issue,
            Path::new("/nonexistent"),
            Path::new("/nonexistent"),
        )
        .unwrap();
        assert_eq!(expanded, "Key: ");
    }

    #[test]
    fn find_skill_tag_finds_builtin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("design.md"), "template").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let result = find_skill_tag(&["design".into()], dir.path(), work_dir.path());
        assert_eq!(result, Some("design".into()));
    }

    #[test]
    fn find_skill_tag_returns_none_for_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        let result = find_skill_tag(&["unknown".into()], dir.path(), work_dir.path());
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_skill_prefers_local() {
        let builtin_dir = tempfile::tempdir().unwrap();
        std::fs::write(builtin_dir.path().join("design.md"), "builtin").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let local_skills = work_dir.path().join(".weaver/skills");
        std::fs::create_dir_all(&local_skills).unwrap();
        std::fs::write(local_skills.join("design.md"), "local").unwrap();

        let runner = AgentRunner {
            api_url: "http://localhost:0".into(),
            workflows_dir: builtin_dir.path().to_path_buf(),
            sdk_dir: PathBuf::from("/nonexistent"),
            binary: "claude".into(),
        };

        let resolved = runner.resolve_skill("design", work_dir.path()).unwrap();
        assert_eq!(resolved, local_skills.join("design.md"));
    }

    #[test]
    fn test_expand_includes_basic() {
        let skills_dir = tempfile::tempdir().unwrap();
        std::fs::write(skills_dir.path().join("_test.md"), "Partial content").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let result = expand_template(
            "{{include:_test}}\nBody",
            &make_test_issue(),
            skills_dir.path(),
            work_dir.path(),
        )
        .unwrap();
        assert!(result.contains("Partial content"));
        assert!(result.contains("Body"));
    }

    #[test]
    fn test_expand_includes_strips_frontmatter() {
        let skills_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            skills_dir.path().join("_test.md"),
            "---\nname: test\n---\nContent after frontmatter",
        )
        .unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let result = expand_template(
            "{{include:_test}}",
            &make_test_issue(),
            skills_dir.path(),
            work_dir.path(),
        )
        .unwrap();
        assert!(result.contains("Content after frontmatter"));
        assert!(!result.contains("---"));
    }

    #[test]
    fn test_expand_includes_not_found() {
        let skills_dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        let result = expand_template(
            "{{include:_nonexistent}}",
            &make_test_issue(),
            skills_dir.path(),
            work_dir.path(),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("_nonexistent"), "error was: {err}");
    }

    #[test]
    fn test_expand_includes_circular() {
        let skills_dir = tempfile::tempdir().unwrap();
        std::fs::write(skills_dir.path().join("_a.md"), "{{include:_b}}").unwrap();
        std::fs::write(skills_dir.path().join("_b.md"), "{{include:_a}}").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let result = expand_template(
            "{{include:_a}}",
            &make_test_issue(),
            skills_dir.path(),
            work_dir.path(),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("circular"), "error was: {err}");
    }

    #[test]
    fn test_expand_includes_recursive() {
        let skills_dir = tempfile::tempdir().unwrap();
        std::fs::write(skills_dir.path().join("_a.md"), "Before {{include:_b}} After").unwrap();
        std::fs::write(skills_dir.path().join("_b.md"), "DEEP").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let result = expand_template(
            "{{include:_a}}",
            &make_test_issue(),
            skills_dir.path(),
            work_dir.path(),
        )
        .unwrap();
        assert!(result.contains("DEEP"));
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
    }

    #[test]
    fn test_expand_includes_then_variables() {
        let skills_dir = tempfile::tempdir().unwrap();
        std::fs::write(skills_dir.path().join("_header.md"), "Title: {{issue_title}}").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let issue = make_test_issue();
        let result = expand_template(
            "{{include:_header}}\nRest of template",
            &issue,
            skills_dir.path(),
            work_dir.path(),
        )
        .unwrap();
        assert!(result.contains(&format!("Title: {}", issue.title)));
    }

    #[test]
    fn test_expand_includes_local_override() {
        let skills_dir = tempfile::tempdir().unwrap();
        std::fs::write(skills_dir.path().join("_test.md"), "BUILTIN").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let local_skills = work_dir.path().join(".weaver/skills");
        std::fs::create_dir_all(&local_skills).unwrap();
        std::fs::write(local_skills.join("_test.md"), "LOCAL").unwrap();

        let result = expand_template(
            "{{include:_test}}",
            &make_test_issue(),
            skills_dir.path(),
            work_dir.path(),
        )
        .unwrap();
        assert!(result.contains("LOCAL"));
        assert!(!result.contains("BUILTIN"));
    }

    #[test]
    fn test_list_skills_filters_partials() {
        let builtins_dir = tempfile::tempdir().unwrap();
        std::fs::write(builtins_dir.path().join("design.md"), "template").unwrap();
        std::fs::write(builtins_dir.path().join("_preamble.md"), "preamble").unwrap();

        let work_dir = tempfile::tempdir().unwrap();
        let skills = list_skills(builtins_dir.path(), work_dir.path());
        let tags: Vec<&str> = skills.iter().map(|s| s.tag.as_str()).collect();
        assert!(tags.contains(&"design"));
        assert!(!tags.contains(&"_preamble"));
    }

    #[test]
    fn parse_stream_line_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc-123","model":"claude-sonnet-4","tools":[]}"#;
        let events = parse_stream_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Init { session_id, model } => {
                assert_eq!(session_id, "abc-123");
                assert_eq!(model, "claude-sonnet-4");
            }
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_line_assistant_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}]}}"#;
        let events = parse_stream_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Text { text } => assert_eq!(text, "Hello world"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_line_assistant_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file":"a.rs"}}]}}"#;
        let events = parse_stream_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolUse { tool, .. } => assert_eq!(tool, "Edit"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_line_result() {
        let line = r#"{"type":"result","subtype":"success","result":"done","usage":{"input_tokens":500,"output_tokens":200},"total_cost_usd":0.05,"model":"claude-sonnet-4"}"#;
        let events = parse_stream_line(line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Result { result, input_tokens, output_tokens, model, cost_usd } => {
                assert_eq!(result, "done");
                assert_eq!(*input_tokens, 500);
                assert_eq!(*output_tokens, 200);
                assert_eq!(model.as_deref(), Some("claude-sonnet-4"));
                assert!((cost_usd - 0.05).abs() < 0.001);
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_line_unknown_type_ignored() {
        let events = parse_stream_line(r#"{"type":"unknown","data":"stuff"}"#);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_stream_line_malformed_json_ignored() {
        let events = parse_stream_line("not json at all");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_stream_line_multiple_content_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"first"},{"type":"tool_use","name":"Bash","input":{"cmd":"ls"}},{"type":"text","text":"second"}]}}"#;
        let events = parse_stream_line(line);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], StreamEvent::Text { text } if text == "first"));
        assert!(matches!(&events[1], StreamEvent::ToolUse { tool, .. } if tool == "Bash"));
        assert!(matches!(&events[2], StreamEvent::Text { text } if text == "second"));
    }

    #[test]
    fn test_parse_skill_template_with_sandbox() {
        let content = "---\nname: research\ndescription: Research task\nsandbox: readonly\n---\nDo research.";
        let template = parse_skill_template(content).unwrap();
        assert_eq!(template.name, "research");
        assert_eq!(template.sandbox, Some("readonly".to_string()));
        assert!(template.body.starts_with("Do research."));
    }

    #[test]
    fn test_parse_skill_template_without_sandbox() {
        let content = "---\nname: design\ndescription: Create a design doc\n---\nYou are a designer.";
        let template = parse_skill_template(content).unwrap();
        assert_eq!(template.sandbox, None);
    }

    fn make_test_issue() -> Issue {
        Issue {
            id: "abcdefgh-1234-5678-9012-ijklmnopqrst".into(),
            title: "Test issue".into(),
            body: "Test body".into(),
            status: crate::issue::IssueStatus::Pending,
            context: serde_json::json!({}),
            dependencies: vec![],
            num_tries: 0,
            max_tries: 3,
            parent_issue_id: None,
            tags: vec![],
            priority: 0,
            channel_kind: None,
            origin_ref: None,
            user_id: None,
            error: None,
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
            claude_session_id: None,
        }
    }

    #[test]
    fn retryable_api_error_500() {
        assert!(is_retryable_api_error(
            r#"API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server error"}}"#
        ));
    }

    #[test]
    fn retryable_api_error_529() {
        assert!(is_retryable_api_error("API Error: 529 overloaded"));
    }

    #[test]
    fn retryable_api_error_overloaded() {
        assert!(is_retryable_api_error("The API is overloaded, please try again"));
    }

    #[test]
    fn non_retryable_error() {
        assert!(!is_retryable_api_error("invalid API key"));
        assert!(!is_retryable_api_error("rate limit exceeded"));
        assert!(!is_retryable_api_error(""));
    }
}
