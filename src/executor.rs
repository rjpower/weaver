use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::db::Db;
use crate::issue::{self, estimate_cost, get_comments, get_issue_tree, Comment, Issue, IssueStatus, UpdateIssueParams, UsageStats};
use crate::runner::{expand_template, find_skill_tag, parse_skill_template, resolve_partial, AgentRunner, SkillTemplate};

/// Hooks called by the executor after issue state transitions.
#[async_trait::async_trait]
pub trait ExecutorHooks: Send + Sync {
    async fn on_issue_completed(&self, _issue: &Issue) {}
    async fn on_issue_failed(&self, _issue: &Issue) {}
    async fn on_issue_awaiting_review(&self, _issue: &Issue) {}
}

pub struct NoopHooks;

#[async_trait::async_trait]
impl ExecutorHooks for NoopHooks {}

/// Reads notification sinks from the settings DB on each event.
pub struct NotifyHooks {
    db: Db,
}

impl NotifyHooks {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl ExecutorHooks for NotifyHooks {
    async fn on_issue_completed(&self, issue: &Issue) {
        let notifier = crate::notify::Notifier::from_db(&self.db).await;
        notifier.notify("issue.completed", issue).await;
    }
    async fn on_issue_failed(&self, issue: &Issue) {
        let notifier = crate::notify::Notifier::from_db(&self.db).await;
        notifier.notify("issue.failed", issue).await;
    }
    async fn on_issue_awaiting_review(&self, issue: &Issue) {
        let notifier = crate::notify::Notifier::from_db(&self.db).await;
        notifier.notify("issue.awaiting_review", issue).await;
    }
}

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub max_concurrent: usize,
    pub poll_interval_secs: u64,
    /// Per-issue execution timeout in seconds. 0 means no timeout.
    pub timeout_secs: u64,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            poll_interval_secs: 2,
            timeout_secs: 2 * 60 * 60, // 2 hours
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RunReport {
    pub executed: Vec<String>,
    pub failed: Vec<String>,
    pub completed: Vec<String>,
}

pub struct Executor {
    db: Db,
    config: ExecutorConfig,
    runner: Arc<AgentRunner>,
    hooks: Arc<dyn ExecutorHooks>,
}

impl Executor {
    pub fn new(db: Db, config: ExecutorConfig, runner: Arc<AgentRunner>) -> Self {
        Self {
            db,
            config,
            runner,
            hooks: Arc::new(NoopHooks),
        }
    }

    pub fn with_hooks(
        db: Db,
        config: ExecutorConfig,
        runner: Arc<AgentRunner>,
        hooks: Arc<dyn ExecutorHooks>,
    ) -> Self {
        Self {
            db,
            config,
            runner,
            hooks,
        }
    }

    /// One-shot: execute all ready issues, then return.
    pub async fn run_once(&self) -> anyhow::Result<RunReport> {
        let mut report = RunReport::default();
        let ready = self.pick_ready_issues().await?;

        let active = self.count_active_issues().await?;
        let slots = self.config.max_concurrent.saturating_sub(active);

        let mut handles = Vec::new();

        for issue_id in ready.into_iter().take(slots) {
            let db = self.db.clone();
            let runner = self.runner.clone();
            let timeout_secs = self.config.timeout_secs;

            let handle = tokio::spawn(async move {
                let result = execute_issue(&db, &runner, &issue_id, timeout_secs).await;
                (issue_id, result)
            });
            handles.push(handle);
        }

        for handle in handles {
            let (issue_id, result) = handle.await?;
            report.executed.push(issue_id.clone());
            match result {
                Ok(()) => {
                    let issue = issue::get_issue(&self.db, &issue_id).await?;
                    match issue.status {
                        IssueStatus::Completed => {
                            self.hooks.on_issue_completed(&issue).await;
                            report.completed.push(issue_id);
                        }
                        IssueStatus::Failed | IssueStatus::ValidationFailed => {
                            self.hooks.on_issue_failed(&issue).await;
                            report.failed.push(issue_id);
                        }
                        IssueStatus::AwaitingReview => {
                            self.hooks.on_issue_awaiting_review(&issue).await;
                        }
                        _ => {}
                    }
                }
                Err(_) => {
                    if let Ok(issue) = issue::get_issue(&self.db, &issue_id).await {
                        self.hooks.on_issue_failed(&issue).await;
                    }
                    report.failed.push(issue_id);
                }
            }
        }

        Ok(report)
    }

    /// Poll the DB until an issue reaches a terminal state.
    pub async fn wait_for_issue(
        &self,
        id: &str,
        cancel: CancellationToken,
    ) -> anyhow::Result<Issue> {
        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.poll_interval_secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = cancel.cancelled() => {
                    return Err(anyhow::anyhow!("wait_for_issue cancelled"));
                }
            }

            let issue = issue::get_issue(&self.db, id).await?;
            if issue.status.is_terminal() {
                return Ok(issue);
            }
        }
    }

    /// Continuously poll for ready issues and execute them.
    pub async fn run_loop(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        // Reset stuck running issues on startup
        let reset = sqlx::query(
            "UPDATE issues SET status = 'failed', error = 'Reset on startup (was running)', \
             completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE status = 'running'",
        )
        .execute(&self.db)
        .await?;
        if reset.rows_affected() > 0 {
            tracing::info!(count = reset.rows_affected(), "Reset stuck running issues");
        }

        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.poll_interval_secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        // GC worktrees every ~5 minutes
        let gc_every_ticks = 300 / self.config.poll_interval_secs.max(1);
        let mut tick_count: u64 = 0;

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = cancel.cancelled() => break,
            }

            tick_count += 1;
            if tick_count % gc_every_ticks == 0 {
                let keep_count = crate::settings::get_known(
                    &self.db,
                    "worktree.keep_count",
                )
                .await
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(32usize);

                match gc_worktrees(&self.db, keep_count).await {
                    Ok(report) if !report.removed.is_empty() => {
                        tracing::info!(
                            removed = report.removed.len(),
                            kept_active = report.kept_active,
                            kept_recent = report.kept_recent,
                            "Worktree GC completed"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Worktree GC failed");
                    }
                    _ => {}
                }
            }

            let ready = self.pick_ready_issues().await?;
            if ready.is_empty() {
                continue;
            }

            // Count running issues that are actively working (not just waiting
            // for children). A coordinator blocked on `weaver issue wait` shouldn't
            // prevent its own children from getting a slot.
            let active = self.count_active_issues().await?;
            let slots = self.config.max_concurrent.saturating_sub(active);
            if slots == 0 {
                continue;
            }

            tracing::info!(count = ready.len(), active, slots, "Picked up ready issues");

            for issue_id in ready.into_iter().take(slots) {
                let db = self.db.clone();
                let runner = self.runner.clone();
                let hooks = self.hooks.clone();
                // Read timeout from settings, falling back to config default
                let timeout_secs = crate::settings::get_known(
                    &self.db,
                    "executor.timeout_secs",
                )
                .await
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(self.config.timeout_secs);
                tokio::spawn(async move {
                    let exec_result = execute_issue(&db, &runner, &issue_id, timeout_secs).await;
                    if let Ok(issue) = issue::get_issue(&db, &issue_id).await {
                        match issue.status {
                            IssueStatus::Completed => hooks.on_issue_completed(&issue).await,
                            IssueStatus::Failed | IssueStatus::ValidationFailed => {
                                hooks.on_issue_failed(&issue).await
                            }
                            IssueStatus::AwaitingReview => {
                                hooks.on_issue_awaiting_review(&issue).await
                            }
                            _ => {}
                        }
                    }
                    if let Err(e) = exec_result {
                        tracing::error!(issue_id, error = %e, "Issue execution failed");
                    }
                });
            }
        }

        Ok(())
    }

    /// Count running issues that are actively working — not waiting for children.
    /// A running issue with pending/running children is just blocked on `weaver issue wait`;
    /// it shouldn't count against the concurrency limit.
    async fn count_active_issues(&self) -> anyhow::Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM issues r \
             WHERE r.status = 'running' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM issues c \
                 WHERE c.parent_issue_id = r.id \
                 AND c.status IN ('pending', 'running') \
             )",
        )
        .fetch_one(&self.db)
        .await?;
        Ok(count as usize)
    }

    async fn pick_ready_issues(&self) -> anyhow::Result<Vec<String>> {
        let pending: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM issues WHERE status = 'pending' ORDER BY priority DESC, created_at ASC",
        )
        .fetch_all(&self.db)
        .await?;

        let mut ready = Vec::new();
        for (issue_id,) in pending {
            if deps_satisfied(&self.db, &issue_id).await? {
                ready.push(issue_id);
            }
        }

        Ok(ready)
    }
}

async fn deps_satisfied(db: &Db, issue_id: &str) -> anyhow::Result<bool> {
    let deps_json: String = sqlx::query_scalar("SELECT dependencies FROM issues WHERE id = ?")
        .bind(issue_id)
        .fetch_one(db)
        .await?;

    let deps: Vec<String> = serde_json::from_str(&deps_json)?;
    if deps.is_empty() {
        return Ok(true);
    }

    for dep_id in &deps {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM issues WHERE id = ?")
                .bind(dep_id)
                .fetch_optional(db)
                .await?;
        match status.as_deref() {
            Some("completed") => {}
            Some("failed") | Some("validation_failed") => {
                tracing::info!(
                    issue_id,
                    dep = dep_id,
                    "Issue blocked by failed dependency"
                );
                issue::update_issue(
                    db,
                    issue_id,
                    UpdateIssueParams {
                        status: Some(IssueStatus::Blocked),
                        error: Some(format!("Dependency {dep_id} failed")),
                        ..Default::default()
                    },
                )
                .await?;
                return Ok(false);
            }
            _ => return Ok(false),
        }
    }

    Ok(true)
}

fn render_issue_tree(issues: &[Issue], root_id: &str, results: &HashMap<String, String>) -> String {
    let mut children_map: HashMap<String, Vec<&Issue>> = HashMap::new();
    for issue in issues {
        if let Some(ref pid) = issue.parent_issue_id {
            children_map.entry(pid.clone()).or_default().push(issue);
        }
    }

    let mut output = String::new();
    fn render(
        map: &HashMap<String, Vec<&Issue>>,
        results: &HashMap<String, String>,
        parent_id: &str,
        depth: usize,
        out: &mut String,
    ) {
        if let Some(children) = map.get(parent_id) {
            for child in children {
                let indent = "  ".repeat(depth);
                let snippet = results.get(&child.id).map(|s| s.as_str())
                    .or(child.error.as_deref())
                    .unwrap_or("(no result)");
                writeln!(
                    out,
                    "{indent}- {title} ({status}): {snippet}",
                    title = child.title,
                    status = child.status
                )
                .ok();
                render(map, results, &child.id, depth + 1, out);
            }
        }
    }
    render(&children_map, results, root_id, 0, &mut output);
    output
}

/// Build the full agent prompt. Always includes: header, body, context,
/// sub-issues, and comment stream. Retry/revision modes prepend their
/// specific sections.
async fn build_issue_prompt(db: &Db, issue: &Issue, resume: bool) -> anyhow::Result<String> {
    let comments = get_comments(db, &issue.id).await?;
    let has_result_comment = comments.iter().any(|c| c.tag.as_deref() == Some("result"));
    let is_revision = has_result_comment;
    let is_retry = issue.num_tries > 0 && !is_revision;

    // On resume, build a short continuation prompt — Claude already has full context
    // from the session, but needs to see any new comments added since it last ran.
    if resume {
        let mut prompt = format!(
            "Continuing work on weaver issue {}.\n",
            issue.id,
        );
        if is_retry {
            writeln!(
                prompt,
                "\nYour previous attempt failed{}. The worktree has your partial changes. Try again.",
                issue.error.as_ref().map(|e| format!(": {e}")).unwrap_or_default()
            )
            .ok();
        }

        // Include all new comments since the last result comment (agent/user/revision).
        // These are comments added after the session ended that Claude hasn't seen yet.
        let last_result_id = comments
            .iter()
            .rev()
            .find(|c| c.tag.as_deref() == Some("result"))
            .map(|c| c.id)
            .unwrap_or(0);

        let new_comments: Vec<&Comment> = comments
            .iter()
            .filter(|c| {
                c.id > last_result_id
                    && c.tag.as_deref() != Some("generated")
                    && c.tag.as_deref() != Some("result")
            })
            .collect();

        if !new_comments.is_empty() {
            prompt.push_str("\n## New comments since last run\n");
            for comment in &new_comments {
                writeln!(prompt, "- [{}] {}", comment.author, comment.body).ok();
            }
        }

        if is_revision {
            prompt.push_str(
                "\nAddress the revision feedback above.\n",
            );
        }

        // Include any auto-review feedback (generated comments from NOT_OK reviews)
        let generated_feedback: Vec<&Comment> = comments
            .iter()
            .filter(|c| {
                c.tag.as_deref() == Some("generated")
                    && c.body.contains("Auto-review feedback")
            })
            .collect();
        for comment in &generated_feedback {
            prompt.push('\n');
            prompt.push_str(&comment.body);
            prompt.push('\n');
        }

        return Ok(prompt);
    }

    let tree = get_issue_tree(db, &issue.id).await?;

    // Header
    let verb = if is_revision {
        "resuming work on"
    } else if is_retry {
        "retrying"
    } else {
        "working on"
    };
    let mut prompt = format!(
        "You are {verb} weaver issue {id}.\n\
         Use `weaver issue show {id}` to examine issue state.\n\n\
         # {title}\n",
        id = issue.id,
        title = issue.title,
    );

    if !issue.body.is_empty() {
        prompt.push('\n');
        prompt.push_str(&issue.body);
        prompt.push('\n');
    }

    // Mode-specific sections
    if is_revision {
        prompt.push_str("\n## Previous work\n\n");
        prompt.push_str("### Your previous result\n");
        if let Some(result_comment) = comments.iter().rev().find(|c| c.tag.as_deref() == Some("result")) {
            prompt.push_str(&result_comment.body);
            prompt.push('\n');
        }

        let feedback_comments: Vec<&Comment> = comments
            .iter()
            .filter(|c| c.tag.as_deref() == Some("revision"))
            .collect();
        prompt.push_str("\n## Revision feedback\n");
        for comment in &feedback_comments {
            writeln!(prompt, "- [{}] {}", comment.author, comment.body).ok();
        }
        prompt.push_str(
            "\nAddress the feedback above. The code from your previous work is already in the worktree.\n",
        );
    } else if is_retry {
        prompt.push_str("\n## Previous attempt\n\n");
        writeln!(
            prompt,
            "This is attempt {} of {}. The previous attempt failed:",
            issue.num_tries + 1,
            issue.max_tries
        )
        .ok();
        if let Some(ref error) = issue.error {
            writeln!(prompt, "Error: {error}").ok();
        }
        prompt.push_str(
            "\nThe worktree contains all file changes from the previous attempt. \
             Inspect what was already done (git status, git log, etc.) before continuing.\n",
        );
    }

    // Completed dependencies: show their results and branches so the agent can
    // inspect or merge their work. Dep branches are NOT auto-merged — the agent
    // must run `git merge <branch>` if it needs the dep's changes in its worktree.
    if !issue.dependencies.is_empty() {
        let mut dep_lines = Vec::new();
        for dep_id in &issue.dependencies {
            if let Ok(dep) = issue::get_issue(db, dep_id).await {
                if dep.status == IssueStatus::Completed {
                    let result = issue::get_result_comment(db, dep_id).await?.unwrap_or_default();
                    let branch = dep.context.get("branch").and_then(|v| v.as_str()).unwrap_or("");
                    dep_lines.push(format!(
                        "### {id}: {title}\nBranch: {branch}\nResult: {result}\n",
                        id = dep_id,
                        title = dep.title,
                        branch = branch,
                        result = if result.is_empty() { "(no result)" } else { &result },
                    ));
                }
            }
        }
        if !dep_lines.is_empty() {
            prompt.push_str("\n## Completed dependencies\n\n");
            prompt.push_str("These issues completed before yours started. Their branches exist in the repo but are NOT automatically merged into your worktree — run `git merge <branch>` if you need their changes.\n\n");
            for line in &dep_lines {
                prompt.push_str(line);
            }
        }
    }

    // Common sections: context, sub-issues, comments (always included)
    let has_context = issue.context.get("branch").is_some()
        || issue.context.get("work_dir").is_some();
    if has_context {
        prompt.push_str("\n## Context\n");
        if let Some(branch) = issue.context.get("branch").and_then(|v| v.as_str()) {
            writeln!(prompt, "Branch: {branch}").ok();
        }
        if let Some(work_dir) = issue.context.get("work_dir").and_then(|v| v.as_str()) {
            writeln!(prompt, "Working directory: {work_dir}").ok();
        }
    }

    if !tree.is_empty() {
        let tree_ids: Vec<String> = tree.iter().map(|i| i.id.clone()).collect();
        let results = issue::get_result_comments_batch(db, &tree_ids).await?;
        prompt.push_str("\n## Sub-issues\n");
        prompt.push_str(&render_issue_tree(&tree, &issue.id, &results));
    }

    let agent_comments: Vec<&Comment> = comments
        .iter()
        .filter(|c| c.tag.is_none())
        .collect();
    if !agent_comments.is_empty() {
        prompt.push_str("\n## Comments\n");
        for comment in &agent_comments {
            writeln!(prompt, "- [{}] {}", comment.author, comment.body).ok();
        }
    }

    Ok(prompt)
}

fn has_explicit_work_dir(issue: &Issue) -> bool {
    issue.context.get("work_dir").and_then(|v| v.as_str()).is_some()
}

/// Auto-create a worktree for a child issue that doesn't have one.
/// Returns the updated issue if a worktree was created, None otherwise.
async fn ensure_worktree(db: &Db, issue: &Issue) -> anyhow::Result<Option<Issue>> {
    if has_explicit_work_dir(issue) {
        return Ok(None);
    }

    if issue.context.get("same_worktree").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(None);
    }

    let parent_id = match &issue.parent_issue_id {
        Some(id) => id,
        None => return Ok(None),
    };

    let parent = issue::get_issue(db, parent_id).await?;
    let base = parent
        .context
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main");

    let branch = format!("work/{}", issue.id);
    let safe_name = branch.replace('/', "-");
    let worktree_dir = PathBuf::from(".weaver/worktrees");
    std::fs::create_dir_all(&worktree_dir)?;
    let worktree_path = worktree_dir.join(&safe_name);

    // Clean up stale worktree references
    tokio::process::Command::new("git")
        .args(["worktree", "prune"])
        .status()
        .await?;

    let status = tokio::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            &branch,
            worktree_path.to_str().unwrap(),
            base,
        ])
        .status()
        .await?;

    if !status.success() {
        // Branch may already exist — try without -b
        let status = tokio::process::Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                &branch,
            ])
            .status()
            .await?;

        if !status.success() {
            anyhow::bail!(
                "failed to create worktree for issue {} (branch '{}')",
                issue.id,
                branch
            );
        }
    }

    let abs_path = std::fs::canonicalize(&worktree_path)?;

    let mut ctx = issue.context.as_object().cloned().unwrap_or_default();
    ctx.insert(
        "work_dir".into(),
        serde_json::json!(abs_path.to_string_lossy()),
    );
    ctx.insert("branch".into(), serde_json::json!(branch));
    ctx.insert("base_branch".into(), serde_json::json!(base));
    issue::update_issue(
        db,
        &issue.id,
        UpdateIssueParams {
            context: Some(serde_json::Value::Object(ctx)),
            ..Default::default()
        },
    )
    .await?;

    // Re-read the issue so the caller gets updated context
    let updated = issue::get_issue(db, &issue.id).await?;
    Ok(Some(updated))
}

/// Result of a worktree garbage collection run.
#[derive(Debug, Default, Serialize)]
pub struct GcReport {
    pub removed: Vec<String>,
    pub kept_active: usize,
    pub kept_recent: usize,
    pub errors: Vec<String>,
}

/// Remove stale worktrees, keeping those belonging to active issues and the
/// most recent `keep_count` terminal ones. Configurable via the
/// `worktree.keep_count` setting (default 32).
pub async fn gc_worktrees(db: &Db, keep_count: usize) -> anyhow::Result<GcReport> {
    gc_worktrees_in(db, keep_count, &PathBuf::from(".weaver/worktrees")).await
}

/// GC worktrees in the given directory. Separated for testing.
async fn gc_worktrees_in(
    db: &Db,
    keep_count: usize,
    worktree_dir: &PathBuf,
) -> anyhow::Result<GcReport> {
    if !worktree_dir.exists() {
        return Ok(GcReport::default());
    }

    // Prune git's internal worktree bookkeeping first
    tokio::process::Command::new("git")
        .args(["worktree", "prune"])
        .status()
        .await
        .ok();

    let entries: Vec<String> = std::fs::read_dir(&worktree_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();

    if entries.is_empty() {
        return Ok(GcReport::default());
    }

    // Fetch all issues that have a work_dir in their context
    let issues_with_completed: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, status, context, completed_at FROM issues \
         WHERE json_extract(context, '$.work_dir') IS NOT NULL",
    )
    .fetch_all(db)
    .await?;

    struct WorktreeInfo {
        dir_name: String,
        issue_id: String,
        is_active: bool,
        completed_at: Option<String>,
    }

    let abs_worktree_dir = std::fs::canonicalize(&worktree_dir)?;
    let mut matched: Vec<WorktreeInfo> = Vec::new();
    let mut unmatched_dirs: Vec<String> = Vec::new();

    for dir_name in &entries {
        let abs_path = abs_worktree_dir.join(dir_name);
        let abs_str = abs_path.to_string_lossy();

        let found = issues_with_completed.iter().find(|(_, _, ctx_str, _)| {
            serde_json::from_str::<serde_json::Value>(ctx_str)
                .ok()
                .and_then(|ctx| ctx.get("work_dir")?.as_str().map(String::from))
                .map(|wd| wd == abs_str.as_ref())
                .unwrap_or(false)
        });

        match found {
            Some((issue_id, status_str, _, completed_at)) => {
                let status: IssueStatus = status_str.parse().unwrap_or(IssueStatus::Failed);
                matched.push(WorktreeInfo {
                    dir_name: dir_name.clone(),
                    issue_id: issue_id.clone(),
                    is_active: !status.is_terminal(),
                    completed_at: completed_at.clone(),
                });
            }
            None => {
                unmatched_dirs.push(dir_name.clone());
            }
        }
    }

    let mut report = GcReport::default();

    // Keep all active worktrees
    let (active, terminal): (Vec<_>, Vec<_>) =
        matched.into_iter().partition(|w| w.is_active);
    report.kept_active = active.len();

    // Sort terminal by completed_at descending — keep the most recent N
    let mut terminal = terminal;
    terminal.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));

    let to_keep = terminal.len().min(keep_count);
    report.kept_recent = to_keep;
    let to_remove: Vec<_> = terminal.into_iter().skip(to_keep).collect();

    // Remove old terminal worktrees
    for wt in &to_remove {
        let path = worktree_dir.join(&wt.dir_name);
        match remove_worktree(&path).await {
            Ok(()) => {
                tracing::info!(dir = %wt.dir_name, issue = %wt.issue_id, "Removed worktree");
                report.removed.push(wt.dir_name.clone());
            }
            Err(e) => {
                let msg = format!("{}: {e}", wt.dir_name);
                tracing::warn!(dir = %wt.dir_name, "Failed to remove worktree: {e}");
                report.errors.push(msg);
            }
        }
    }

    // Skip unmatched directories — they may belong to other tools or manual work.
    if !unmatched_dirs.is_empty() {
        tracing::debug!(count = unmatched_dirs.len(), "Skipping unmatched worktree dirs (not in DB)");
    }

    Ok(report)
}

async fn remove_worktree(path: &Path) -> anyhow::Result<()> {
    // Try git worktree remove first (cleans up git's bookkeeping)
    let status = tokio::process::Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            path.to_str().unwrap_or_default(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match status {
        Ok(s) if s.success() => return Ok(()),
        _ => {}
    }

    // Fall back to direct removal if git worktree remove fails
    tokio::fs::remove_dir_all(path).await?;
    Ok(())
}

async fn auto_commit_changes(work_dir: &Path, issue_id: &str, title: &str) {
    let status = tokio::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(work_dir)
        .output()
        .await;

    let status = match status {
        Ok(s) if s.status.success() => String::from_utf8_lossy(&s.stdout).to_string(),
        _ => return,
    };

    if status.trim().is_empty() {
        return;
    }

    tracing::info!(issue_id, "Auto-committing uncommitted changes");

    let add = tokio::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(work_dir)
        .output()
        .await;

    if add.is_ok() {
        let msg = format!("wip: {title} [{issue_id}]");
        tokio::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", &msg])
            .current_dir(work_dir)
            .output()
            .await
            .ok();
    }
}

/// Spawns a review child issue for an issue with the `auto-review` tag.
/// The review gets the original human prompt (issue body) so it can assess
/// whether the implementation meets the requirements.
/// Returns the review issue ID.
async fn spawn_auto_review(db: &Db, issue: &Issue) -> anyhow::Result<String> {
    let review_title = format!("review: {}", issue.title);
    let base = issue
        .context
        .get("base_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main");
    let review_body = format!(
        "Review the implementation on this branch against the original task.\n\n\
         ## Original task\n\n{}\n\n\
         ## Instructions\n\n\
         Review the diff (`git diff {base}..HEAD`) against the requirements above.\n\
         End your review with exactly one of these verdicts on its own line:\n\
         - `OK` — implementation meets the requirements\n\
         - `NOT_OK` — implementation has issues that need fixing\n\n\
         If NOT_OK, list specific issues with file:line references.",
        issue.body
    );

    let mut context = serde_json::Map::new();
    context.insert("same_worktree".into(), serde_json::json!(true));
    if let Some(work_dir) = issue.context.get("work_dir") {
        context.insert("work_dir".into(), work_dir.clone());
    }
    if let Some(branch) = issue.context.get("branch") {
        context.insert("branch".into(), branch.clone());
    }

    let review = issue::create_issue(
        db,
        issue::CreateIssueParams {
            title: review_title,
            body: Some(review_body),
            tags: vec!["review".to_string()],
            parent_issue_id: Some(issue.id.clone()),
            context: Some(serde_json::Value::Object(context)),
            ..Default::default()
        },
    )
    .await?;

    issue::add_comment(
        db,
        &issue.id,
        "weaver",
        &format!("Auto-review spawned: {} ({})", review.id, review.title),
        Some("generated"),
    )
    .await?;

    tracing::info!(
        issue_id = issue.id,
        review_id = review.id,
        "Spawned auto-review child"
    );

    Ok(review.id)
}

/// Find a terminal review child issue (spawned by auto-review).
/// Returns both completed and failed reviews — a failed review (infrastructure error)
/// should not permanently block the parent.
async fn find_terminal_review_child(db: &Db, parent_id: &str) -> anyhow::Result<Option<Issue>> {
    let children = issue::list_issues(
        db,
        issue::ListFilter {
            scope: issue::IssueScope::ChildrenOf(parent_id.to_string()),
            tag: Some("review".to_string()),
            ..Default::default()
        },
    )
    .await?
    .issues;

    Ok(children
        .into_iter()
        .find(|c| c.status == IssueStatus::Completed || c.status == IssueStatus::Failed))
}

/// Returns true if the review result indicates approval.
/// Checks for LGTM/OK verdicts (the skill template asks for LGTM/CHANGES_NEEDED,
/// but agents sometimes use OK/NOT_OK or markdown formatting).
fn check_auto_review_result(review_comments: &[Comment]) -> bool {
    if let Some(result) = review_comments.iter().rev().find(|c| c.tag.as_deref() == Some("result")) {
        let text = result.body.to_uppercase();
        // Strip markdown bold markers for matching
        let text = text.replace("**", "");

        // Negative verdicts — check these first since "NOT_OK" contains "OK"
        let dominated_search = ["NOT_OK", "CHANGES_NEEDED"];
        for needle in dominated_search {
            if text.contains(needle) {
                return false;
            }
        }
        // Positive verdicts
        let pass = ["LGTM", "OK"];
        for needle in pass {
            if text.contains(needle) {
                return true;
            }
        }
        false
    } else {
        false
    }
}

/// Apply sandbox level from skill template to issue context, unless already set.
fn inject_skill_sandbox(context: &mut serde_json::Value, template: &SkillTemplate) {
    if let Some(ref sb) = template.sandbox {
        if context.get("sandbox").is_none() {
            if let Some(obj) = context.as_object_mut() {
                obj.insert("sandbox".into(), serde_json::json!(sb));
            }
        }
    }
}

async fn execute_issue(
    db: &Db,
    runner: &Arc<AgentRunner>,
    issue_id: &str,
    timeout_secs: u64,
) -> anyhow::Result<()> {
    let issue = issue::get_issue(db, issue_id).await?;

    if issue.status == IssueStatus::Completed || issue.status == IssueStatus::Failed {
        return Ok(());
    }

    // Auto-review resolution: if this issue was re-queued after an auto-review,
    // check if the review passed. If OK, strip the tag and complete without
    // re-running the agent. If NOT_OK, the agent will run again with review
    // feedback already attached as a comment.
    if issue.tags.contains(&"auto-review".to_string()) {
        let parent_comments = issue::get_comments(db, issue_id).await?;
        let last_revision_at = parent_comments
            .iter()
            .filter(|c| c.tag.as_deref() == Some("revision"))
            .map(|c| c.created_at.as_str())
            .max()
            .map(String::from);

        if let Some(review_result) = find_terminal_review_child(db, issue_id).await? {
            // If the review predates a revision comment, it's stale — skip and re-run the agent.
            let review_is_stale = last_revision_at
                .as_deref()
                .map(|rev_at| review_result.created_at.as_str() < rev_at)
                .unwrap_or(false);

            if review_is_stale {
                tracing::info!(issue_id, "Auto-review child is stale (predates revision), re-running agent");
                // Fall through to agent execution below.
            } else {
                // If the review agent itself failed (infrastructure error), treat as pass
                if review_result.status == IssueStatus::Failed {
                    let mut tags = issue.tags.clone();
                    tags.retain(|t| t != "auto-review");
                    issue::update_issue(
                        db,
                        issue_id,
                        UpdateIssueParams {
                            status: Some(IssueStatus::Completed),
                            tags: Some(tags),
                            ..Default::default()
                        },
                    )
                    .await?;
                    issue::add_comment(db, issue_id, "weaver", "Auto-review agent failed — treating as pass.", Some("generated")).await?;
                    tracing::warn!(issue_id, "Auto-review agent failed, completing parent anyway");
                    return Ok(());
                }

                let review_comments = issue::get_comments(db, &review_result.id).await?;
                if check_auto_review_result(&review_comments) {
                    // Review passed — strip auto-review tag and complete
                    let mut tags = issue.tags.clone();
                    tags.retain(|t| t != "auto-review");
                    issue::update_issue(
                        db,
                        issue_id,
                        UpdateIssueParams {
                            status: Some(IssueStatus::Completed),
                            tags: Some(tags),
                            ..Default::default()
                        },
                    )
                    .await?;
                    issue::add_comment(db, issue_id, "weaver", "Auto-review passed (OK). Completing.", Some("generated")).await?;
                    tracing::info!(issue_id, "Auto-review passed, completing issue");
                    return Ok(());
                }
                // Review failed — add feedback and let the agent run again
                if let Some(result_comment) = review_comments.iter().rev().find(|c| c.tag.as_deref() == Some("result")) {
                    issue::add_comment(
                        db,
                        issue_id,
                        "weaver",
                        &format!("Auto-review feedback (NOT_OK):\n{}", result_comment.body),
                        Some("generated"),
                    ).await?;
                }
                tracing::info!(issue_id, "Auto-review failed, re-running agent with feedback");
            }
        }
    }

    // Auto-create a worktree for child issues that don't have one
    let issue = match ensure_worktree(db, &issue).await? {
        Some(updated) => updated,
        None => issue,
    };

    let work_dir = issue
        .context
        .get("work_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Set running, increment tries
    tracing::info!(
        issue_id,
        title = issue.title,
        "Starting issue"
    );
    issue::update_issue(
        db,
        issue_id,
        UpdateIssueParams {
            status: Some(IssueStatus::Running),
            num_tries: Some(issue.num_tries + 1),
            ..Default::default()
        },
    )
    .await?;

    // Clear events from previous attempts so seq numbers don't overlap
    issue::clear_events(db, issue_id).await?;

    // Create event channel for streaming agent output to DB
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<crate::runner::StreamEvent>(64);
    let db_for_events = db.clone();
    let issue_id_for_events = issue_id.to_string();
    let event_forwarder = tokio::spawn(async move {
        let mut seq = 0i64;
        while let Some(event) = event_rx.recv().await {
            seq += 1;
            issue::insert_event(&db_for_events, &issue_id_for_events, seq, &event)
                .await
                .ok();
        }
    });

    // Determine if this is a resume (we have a previous session to continue)
    let is_resume = issue.claude_session_id.is_some();

    let agent_future = async {
        let mut context = issue.context.clone();

        // Inject resume_id into context if we have a previous session
        if let Some(ref session_id) = issue.claude_session_id {
            if let Some(obj) = context.as_object_mut() {
                obj.insert("resume_id".into(), serde_json::json!(session_id));
            }
        }

        // Resolve skill template (needed for fresh runs and resume fallback)
        let resolve_skill_body = |ctx: &mut serde_json::Value| -> anyhow::Result<Option<String>> {
            if let Some(skill_tag) = find_skill_tag(&issue.tags, runner.skills_dir(), &work_dir) {
                let path = runner.resolve_skill(&skill_tag, &work_dir).ok_or_else(|| {
                    anyhow::anyhow!("skill template not found for tag: {skill_tag}")
                })?;
                let content = std::fs::read_to_string(&path)?;
                let template = parse_skill_template(&content)?;
                inject_skill_sandbox(ctx, &template);
                Ok(Some(expand_template(&template.body, &issue, runner.skills_dir(), &work_dir)?))
            } else if let Some(default_path) = resolve_partial("_default", runner.skills_dir(), &work_dir) {
                let content = std::fs::read_to_string(&default_path)?;
                let template = parse_skill_template(&content)?;
                inject_skill_sandbox(ctx, &template);
                Ok(Some(expand_template(&template.body, &issue, runner.skills_dir(), &work_dir)?))
            } else {
                Ok(None)
            }
        };

        // On resume: skip skill body (Claude already has it), use shorter prompt
        let skill_body = if is_resume {
            // Still resolve for sandbox injection, but don't send as system prompt
            resolve_skill_body(&mut context)?;
            None
        } else {
            resolve_skill_body(&mut context)?
        };

        let prompt = build_issue_prompt(db, &issue, is_resume).await?;

        // Log the full prompt so it's visible in the UI
        let mut prompt_parts = Vec::new();
        if is_resume {
            prompt_parts.push("[resuming session]".to_string());
        }
        if let Some(ref skill) = skill_body {
            prompt_parts.push(format!("[skill template]\n{skill}"));
        }
        prompt_parts.push(format!("[prompt]\n{prompt}"));
        let full_prompt = prompt_parts.join("\n\n");
        issue::add_comment(db, issue_id, "prompt", &full_prompt, Some("generated")).await.ok();

        let result = runner
            .call_agent(&prompt, &work_dir, context.clone(), skill_body.as_deref(), issue_id, Some(event_tx.clone()))
            .await;

        // On resume failure, fall back to fresh execution
        match (&result, is_resume) {
            (Err(e), true) => {
                tracing::warn!(issue_id, error = %e, "Resume failed, falling back to fresh execution");
                issue::set_claude_session_id(db, issue_id, "").await.ok();
                issue::clear_events(db, issue_id).await.ok();

                // Remove resume_id from context
                if let Some(obj) = context.as_object_mut() {
                    obj.remove("resume_id");
                }

                // Rebuild with full skill body and prompt
                let fresh_skill_body = resolve_skill_body(&mut context)?;
                let fresh_prompt = build_issue_prompt(db, &issue, false).await?;

                issue::add_comment(
                    db, issue_id, "prompt",
                    &format!("[resume fallback]\n\n[prompt]\n{fresh_prompt}"),
                    Some("generated"),
                ).await.ok();

                runner
                    .call_agent(&fresh_prompt, &work_dir, context, fresh_skill_body.as_deref(), issue_id, Some(event_tx))
                    .await
            }
            _ => result,
        }
    };

    let result = if timeout_secs > 0 {
        match tokio::time::timeout(Duration::from_secs(timeout_secs), agent_future).await {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!(
                "execution timed out after {timeout_secs}s"
            )),
        }
    } else {
        agent_future.await
    };

    // Wait for the event forwarder to drain all remaining events to DB
    event_forwarder.await.ok();

    match result {
        Ok(agent_result) => {
            tracing::info!(
                issue_id,
                title = issue.title,
                input_tokens = agent_result.input_tokens,
                output_tokens = agent_result.output_tokens,
                "Issue completed"
            );
            let preview = match agent_result.result.char_indices().nth(200) {
                Some((idx, _)) => &agent_result.result[..idx],
                None => agent_result.result.as_str(),
            };
            tracing::info!(issue_id, "  result: {preview}");

            // Store Claude session ID for future --resume
            if let Some(ref sid) = agent_result.session_id {
                issue::set_claude_session_id(db, issue_id, sid).await.ok();
            }

            // Record usage
            let cost = estimate_cost(
                agent_result.input_tokens,
                agent_result.output_tokens,
                agent_result.model.as_deref(),
            );
            issue::insert_usage(
                db,
                issue_id,
                &UsageStats {
                    input_tokens: agent_result.input_tokens,
                    output_tokens: agent_result.output_tokens,
                    model: agent_result.model,
                    cost_usd: Some(cost),
                },
            )
            .await
            .ok();

            // Store result as tagged comment
            issue::add_comment(db, issue_id, "agent", &agent_result.result, Some("result")).await?;

            // Auto-commit any uncommitted changes the agent left behind
            // Only when the issue has an explicit worktree (not the fallback cwd)
            if has_explicit_work_dir(&issue) {
                auto_commit_changes(&work_dir, issue_id, &issue.title).await;
            }

            let current = issue::get_issue(db, issue_id).await?;
            if current.status != IssueStatus::AwaitingReview {
                // Auto-review gate: spawn a review child and re-queue the parent.
                // The review child runs, then when the parent is picked up again,
                // the auto-review resolution at the top of execute_issue handles
                // the OK/NOT_OK decision.
                if current.tags.contains(&"auto-review".to_string()) {
                    let review_id = spawn_auto_review(db, &current).await?;
                    // Add review as a dependency so parent waits for it
                    let mut deps = current.dependencies.clone();
                    deps.push(review_id);
                    issue::update_issue(
                        db,
                        issue_id,
                        UpdateIssueParams {
                            status: Some(IssueStatus::Pending),
                            dependencies: Some(deps),
                            ..Default::default()
                        },
                    )
                    .await?;
                } else {
                    issue::update_issue(
                        db,
                        issue_id,
                        UpdateIssueParams {
                            status: Some(IssueStatus::Completed),
                            ..Default::default()
                        },
                    )
                    .await?;
                }
            }
        }
        Err(e) => {
            // If the agent explicitly requested review before failing, honor it
            if let Ok(current) = issue::get_issue(db, issue_id).await {
                if current.status == IssueStatus::AwaitingReview {
                    return Ok(());
                }
            }
            let error_msg = e.to_string();
            issue::add_comment(db, issue_id, "weaver", &format!("Execution failed: {error_msg}"), Some("generated"))
                .await?;

            // Auto-commit any partial work for retry visibility
            if has_explicit_work_dir(&issue) {
                auto_commit_changes(&work_dir, issue_id, &issue.title).await;
            }

            let tries = issue.num_tries + 1;
            if tries < issue.max_tries {
                tracing::info!(
                    issue_id,
                    try_num = tries,
                    max = issue.max_tries,
                    "Retrying issue"
                );
                issue::update_issue(
                    db,
                    issue_id,
                    UpdateIssueParams {
                        status: Some(IssueStatus::Pending),
                        error: Some(error_msg),
                        ..Default::default()
                    },
                )
                .await?;
            } else {
                tracing::warn!(
                    issue_id,
                    title = issue.title,
                    error = %error_msg,
                    "Issue failed"
                );
                issue::update_issue(
                    db,
                    issue_id,
                    UpdateIssueParams {
                        status: Some(IssueStatus::Failed),
                        error: Some(error_msg),
                        ..Default::default()
                    },
                )
                .await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue::CreateIssueParams;

    fn test_runner() -> Arc<AgentRunner> {
        Arc::new(AgentRunner {
            api_url: "http://localhost:0".into(),
            workflows_dir: PathBuf::from("/nonexistent"),
            sdk_dir: PathBuf::from("/nonexistent"),
            binary: "/nonexistent/binary".into(),
        })
    }

    async fn test_db() -> Db {
        crate::db::connect_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn execute_issue_without_workflow_calls_agent() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Agent task".into(),
                body: Some("Do something".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let runner = test_runner();
        execute_issue(&db, &runner, &issue.id, 0).await.unwrap();

        let updated = issue::get_issue(&db, &issue.id).await.unwrap();
        // Fails because the claude binary doesn't exist in test
        assert!(
            updated.status == IssueStatus::Failed || updated.status == IssueStatus::Pending,
            "Expected failed or pending, got {:?}",
            updated.status
        );
    }

    #[tokio::test]
    async fn deps_block_execution() {
        let db = test_db().await;

        let dep = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Dependency".into(),

                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Child".into(),

                dependencies: vec![dep.id.clone()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Child should not be ready yet
        assert!(!deps_satisfied(&db, &child.id).await.unwrap());
    }

    #[tokio::test]
    async fn dep_failure_blocks_child() {
        let db = test_db().await;

        let dep = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Will fail dep".into(),

                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Child".into(),

                dependencies: vec![dep.id.clone()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Mark dep as failed
        issue::update_issue(
            &db,
            &dep.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Failed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Child should be blocked
        assert!(!deps_satisfied(&db, &child.id).await.unwrap());
        let child_updated = issue::get_issue(&db, &child.id).await.unwrap();
        assert_eq!(child_updated.status, IssueStatus::Blocked);
    }

    #[tokio::test]
    async fn wait_for_issue_returns_on_terminal_status() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Will complete".into(),

                ..Default::default()
            },
        )
        .await
        .unwrap();

        let issue_id = issue.id.clone();
        let db_clone = db.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            issue::update_issue(
                &db_clone,
                &issue_id,
                UpdateIssueParams {
                    status: Some(IssueStatus::Completed),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        });

        let runner = test_runner();
        let executor = Executor::new(
            db,
            ExecutorConfig {
                poll_interval_secs: 1,
                ..Default::default()
            },
            runner,
        );

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            executor.wait_for_issue(&issue.id, CancellationToken::new()),
        )
        .await
        .expect("wait_for_issue should return promptly")
        .unwrap();

        assert_eq!(result.status, IssueStatus::Completed);
    }

    #[tokio::test]
    async fn wait_for_issue_respects_cancellation() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Never finishes".into(),

                ..Default::default()
            },
        )
        .await
        .unwrap();

        let runner = test_runner();
        let executor = Executor::new(
            db,
            ExecutorConfig {
                poll_interval_secs: 1,
                ..Default::default()
            },
            runner,
        );

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_clone.cancel();
        });

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            executor.wait_for_issue(&issue.id, cancel),
        )
        .await
        .expect("wait_for_issue should exit promptly on cancel");

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn count_active_excludes_waiting_parents() {
        let db = test_db().await;
        let runner = test_runner();
        let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);

        // No issues → active = 0
        assert_eq!(executor.count_active_issues().await.unwrap(), 0);

        // Create a running issue with no children → active = 1
        let parent = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Parent coordinator".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        issue::update_issue(
            &db,
            &parent.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Running),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(executor.count_active_issues().await.unwrap(), 1);

        // Add a pending child → parent is now "waiting", active = 0
        let child = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Child task".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(executor.count_active_issues().await.unwrap(), 0);

        // Set child to running → parent still waiting, child is active leaf = 1
        issue::update_issue(
            &db,
            &child.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Running),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(executor.count_active_issues().await.unwrap(), 1);

        // Complete child → parent is active again = 1
        issue::update_issue(
            &db,
            &child.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(executor.count_active_issues().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn count_active_deep_chain_only_counts_leaves() {
        let db = test_db().await;
        let runner = test_runner();
        let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);

        // Build a 3-deep chain: grandparent → parent → child (all running)
        // Only the leaf (child) has no pending/running children, so active = 1
        let gp = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Grandparent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let p = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Parent".into(),
                parent_issue_id: Some(gp.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let c = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Child".into(),
                parent_issue_id: Some(p.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        for id in [&gp.id, &p.id, &c.id] {
            issue::update_issue(
                &db,
                id,
                UpdateIssueParams {
                    status: Some(IssueStatus::Running),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        // 3 running, but only the leaf is active
        assert_eq!(executor.count_active_issues().await.unwrap(), 1);

        // Add a pending grandchild under child → now nobody is active
        let _gc = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Grandchild".into(),
                parent_issue_id: Some(c.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(executor.count_active_issues().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn run_loop_exits_on_cancel() {
        let db = test_db().await;
        let runner = test_runner();
        let executor = Executor::new(
            db,
            ExecutorConfig {
                poll_interval_secs: 1,
                ..Default::default()
            },
            runner,
        );

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let handle = tokio::spawn(async move { executor.run_loop(cancel_clone).await });

        cancel.cancel();

        let result = tokio::time::timeout(Duration::from_secs(3), handle)
            .await
            .expect("run_loop should exit promptly on cancellation");
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn ensure_worktree_skips_when_work_dir_present() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Has work_dir".into(),
                context: Some(serde_json::json!({"work_dir": "/some/path"})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = ensure_worktree(&db, &issue).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn ensure_worktree_skips_when_same_worktree_flag_set() {
        let db = test_db().await;
        let parent = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Parent".into(),
                context: Some(serde_json::json!({"work_dir": "/parent/dir", "branch": "feat/x"})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Child with same_worktree=true inherits parent's work_dir via issue::create_issue,
        // but even if it somehow didn't, ensure_worktree should skip due to the flag.
        let child = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Child same worktree".into(),
                parent_issue_id: Some(parent.id.clone()),
                context: Some(serde_json::json!({"same_worktree": true})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = ensure_worktree(&db, &child).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn ensure_worktree_skips_top_level_issues() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Top-level".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = ensure_worktree(&db, &issue).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn spawn_auto_review_creates_child_issue() {
        let db = test_db().await;

        let parent = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Parent task".into(),
                tags: vec!["auto-review".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        spawn_auto_review(&db, &parent).await.unwrap();

        // Should have created a review child
        let children = issue::list_issues(
            &db,
            issue::ListFilter {
                scope: issue::IssueScope::ChildrenOf(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .issues;

        assert_eq!(children.len(), 1);
        assert_eq!(children[0].tags, vec!["review"]);
        assert!(children[0].title.starts_with("review: "));
        assert_eq!(children[0].parent_issue_id.as_deref(), Some(parent.id.as_str()));

        // Should have a comment on the parent
        let comments = issue::get_comments(&db, &parent.id).await.unwrap();
        assert!(comments.iter().any(|c| c.body.contains("Auto-review spawned")));

        // Review body should contain the original issue body
        assert!(children[0].body.contains("Parent task") || children[0].body.contains("Original task"));
    }

    #[test]
    fn check_auto_review_ok() {
        let comments = vec![Comment {
            id: 1,
            issue_id: "i1".into(),
            author: "agent".into(),
            body: "Everything looks good.\n\nOK".into(),
            tag: Some("result".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        assert!(check_auto_review_result(&comments));
    }

    #[test]
    fn check_auto_review_not_ok() {
        let comments = vec![Comment {
            id: 1,
            issue_id: "i1".into(),
            author: "agent".into(),
            body: "Found issues:\n- Missing test\n\nNOT_OK".into(),
            tag: Some("result".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        assert!(!check_auto_review_result(&comments));
    }

    #[test]
    fn check_auto_review_markdown_bold_ok() {
        let comments = vec![Comment {
            id: 1,
            issue_id: "i1".into(),
            author: "agent".into(),
            body: "The review is complete. Verdict: **OK** — implementation meets requirements.".into(),
            tag: Some("result".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        assert!(check_auto_review_result(&comments));
    }

    #[test]
    fn check_auto_review_lgtm() {
        let comments = vec![Comment {
            id: 1,
            issue_id: "i1".into(),
            author: "agent".into(),
            body: "All tests pass.\n\nLGTM".into(),
            tag: Some("result".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        assert!(check_auto_review_result(&comments));
    }

    #[test]
    fn check_auto_review_changes_needed() {
        let comments = vec![Comment {
            id: 1,
            issue_id: "i1".into(),
            author: "agent".into(),
            body: "**CHANGES_NEEDED**\n\n1. Missing test".into(),
            tag: Some("result".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        assert!(!check_auto_review_result(&comments));
    }

    #[test]
    fn check_auto_review_no_result() {
        let comments = vec![Comment {
            id: 1,
            issue_id: "i1".into(),
            author: "agent".into(),
            body: "some progress".into(),
            tag: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        assert!(!check_auto_review_result(&comments));
    }

    #[tokio::test]
    async fn find_terminal_review_child_returns_none_when_no_children() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "No children".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = find_terminal_review_child(&db, &issue.id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn find_terminal_review_child_returns_completed_review() {
        let db = test_db().await;
        let parent = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Parent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let review = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "review: Parent".into(),
                tags: vec!["review".into()],
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Not completed yet
        let result = find_terminal_review_child(&db, &parent.id).await.unwrap();
        assert!(result.is_none());

        // Mark completed
        issue::update_issue(
            &db,
            &review.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = find_terminal_review_child(&db, &parent.id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, review.id);
    }

    #[tokio::test]
    async fn find_terminal_review_child_returns_most_recent() {
        let db = test_db().await;
        let parent = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Parent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // First review (older, NOT_OK)
        let review1 = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "review: Parent (1)".into(),
                tags: vec!["review".into()],
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            &db,
            &review1.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Second review (newer, OK)
        let review2 = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "review: Parent (2)".into(),
                tags: vec!["review".into()],
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            &db,
            &review2.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Should find the most recent review (review2), not the oldest (review1)
        let result = find_terminal_review_child(&db, &parent.id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, review2.id);
    }

    /// Helper to create a fake worktree dir and issue with matching work_dir context.
    async fn create_worktree_issue(
        db: &Db,
        worktree_base: &Path,
        dir_name: &str,
        status: IssueStatus,
        completed_at: Option<&str>,
    ) -> Issue {
        let dir_path = worktree_base.join(dir_name);
        std::fs::create_dir_all(&dir_path).unwrap();
        let abs_path = std::fs::canonicalize(&dir_path).unwrap();

        let issue = issue::create_issue(
            db,
            CreateIssueParams {
                title: format!("issue for {dir_name}"),
                context: Some(serde_json::json!({
                    "work_dir": abs_path.to_string_lossy(),
                    "branch": format!("work/{dir_name}"),
                })),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            db,
            &issue.id,
            UpdateIssueParams {
                status: Some(status),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Override completed_at if provided (for sorting tests)
        if let Some(ts) = completed_at {
            sqlx::query("UPDATE issues SET completed_at = ? WHERE id = ?")
                .bind(ts)
                .bind(&issue.id)
                .execute(db)
                .await
                .unwrap();
        }

        issue::get_issue(db, &issue.id).await.unwrap()
    }

    #[tokio::test]
    async fn gc_worktrees_empty_dir() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_dir = tmp.path().join("worktrees");
        std::fs::create_dir_all(&wt_dir).unwrap();

        let report = gc_worktrees_in(&db, 10, &wt_dir.to_path_buf()).await.unwrap();
        assert!(report.removed.is_empty());
        assert_eq!(report.kept_active, 0);
        assert_eq!(report.kept_recent, 0);
    }

    #[tokio::test]
    async fn gc_worktrees_nonexistent_dir() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_dir = tmp.path().join("no-such-dir");

        let report = gc_worktrees_in(&db, 10, &wt_dir.to_path_buf()).await.unwrap();
        assert!(report.removed.is_empty());
    }

    #[tokio::test]
    async fn gc_keeps_active_worktrees() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_dir = tmp.path().join("worktrees");
        std::fs::create_dir_all(&wt_dir).unwrap();

        // Running issue — should be kept
        create_worktree_issue(&db, &wt_dir, "work-active1", IssueStatus::Running, None).await;
        // Pending issue — should be kept
        create_worktree_issue(&db, &wt_dir, "work-active2", IssueStatus::Pending, None).await;

        let report = gc_worktrees_in(&db, 0, &wt_dir.to_path_buf()).await.unwrap();
        assert!(report.removed.is_empty());
        assert_eq!(report.kept_active, 2);
    }

    #[tokio::test]
    async fn gc_removes_old_terminal_worktrees() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_dir = tmp.path().join("worktrees");
        std::fs::create_dir_all(&wt_dir).unwrap();

        // Create 3 completed worktrees with different completed_at times
        create_worktree_issue(
            &db, &wt_dir, "work-old", IssueStatus::Completed,
            Some("2026-01-01T00:00:00Z"),
        ).await;
        create_worktree_issue(
            &db, &wt_dir, "work-mid", IssueStatus::Completed,
            Some("2026-02-01T00:00:00Z"),
        ).await;
        create_worktree_issue(
            &db, &wt_dir, "work-new", IssueStatus::Completed,
            Some("2026-03-01T00:00:00Z"),
        ).await;

        // Keep only 1 → should remove 2
        let report = gc_worktrees_in(&db, 1, &wt_dir.to_path_buf()).await.unwrap();
        assert_eq!(report.removed.len(), 2);
        assert_eq!(report.kept_recent, 1);
        assert!(report.removed.contains(&"work-old".to_string()));
        assert!(report.removed.contains(&"work-mid".to_string()));
        // The newest should still exist on disk
        assert!(wt_dir.join("work-new").exists());
        assert!(!wt_dir.join("work-old").exists());
        assert!(!wt_dir.join("work-mid").exists());
    }

    #[tokio::test]
    async fn gc_skips_orphan_directories() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_dir = tmp.path().join("worktrees");
        std::fs::create_dir_all(&wt_dir).unwrap();

        // Create a directory with no matching issue — should be left alone
        std::fs::create_dir_all(wt_dir.join("work-orphan")).unwrap();

        let report = gc_worktrees_in(&db, 10, &wt_dir.to_path_buf()).await.unwrap();
        assert_eq!(report.removed.len(), 0);
        assert!(wt_dir.join("work-orphan").exists());
    }

    #[tokio::test]
    async fn gc_mixed_active_terminal_and_orphan() {
        let db = test_db().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_dir = tmp.path().join("worktrees");
        std::fs::create_dir_all(&wt_dir).unwrap();

        // Active — kept
        create_worktree_issue(&db, &wt_dir, "work-running", IssueStatus::Running, None).await;
        // Recent completed — kept
        create_worktree_issue(
            &db, &wt_dir, "work-recent", IssueStatus::Completed,
            Some("2026-03-01T00:00:00Z"),
        ).await;
        // Old completed — removed
        create_worktree_issue(
            &db, &wt_dir, "work-old", IssueStatus::Completed,
            Some("2026-01-01T00:00:00Z"),
        ).await;
        // Orphan — skipped (not in DB, leave it alone)
        std::fs::create_dir_all(wt_dir.join("work-ghost")).unwrap();

        let report = gc_worktrees_in(&db, 1, &wt_dir.to_path_buf()).await.unwrap();
        assert_eq!(report.kept_active, 1);
        assert_eq!(report.kept_recent, 1);
        assert_eq!(report.removed.len(), 1);
        assert!(report.removed.contains(&"work-old".to_string()));
        // Active, recent, and orphan should still exist
        assert!(wt_dir.join("work-running").exists());
        assert!(wt_dir.join("work-recent").exists());
        assert!(wt_dir.join("work-ghost").exists());
    }

    #[test]
    fn inject_skill_sandbox_sets_when_absent() {
        let mut context = serde_json::json!({});
        let template = crate::runner::SkillTemplate {
            name: "test".into(),
            description: String::new(),
            sandbox: Some("readonly".into()),
            body: String::new(),
        };
        inject_skill_sandbox(&mut context, &template);
        assert_eq!(context["sandbox"], "readonly");
    }

    #[test]
    fn inject_skill_sandbox_preserves_existing() {
        let mut context = serde_json::json!({"sandbox": "unrestricted"});
        let template = crate::runner::SkillTemplate {
            name: "test".into(),
            description: String::new(),
            sandbox: Some("readonly".into()),
            body: String::new(),
        };
        inject_skill_sandbox(&mut context, &template);
        assert_eq!(context["sandbox"], "unrestricted");
    }

    #[test]
    fn inject_skill_sandbox_noop_when_template_has_none() {
        let mut context = serde_json::json!({});
        let template = crate::runner::SkillTemplate {
            name: "test".into(),
            description: String::new(),
            sandbox: None,
            body: String::new(),
        };
        inject_skill_sandbox(&mut context, &template);
        assert!(context.get("sandbox").is_none());
    }

    #[tokio::test]
    async fn set_and_get_claude_session_id() {
        let db = test_db().await;
        let created = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Session ID test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(created.claude_session_id.is_none());

        issue::set_claude_session_id(&db, &created.id, "sess-abc-123").await.unwrap();
        let updated = issue::get_issue(&db, &created.id).await.unwrap();
        assert_eq!(updated.claude_session_id.as_deref(), Some("sess-abc-123"));

        // Clearing session_id
        issue::set_claude_session_id(&db, &created.id, "").await.unwrap();
        let cleared = issue::get_issue(&db, &created.id).await.unwrap();
        assert!(cleared.claude_session_id.is_none());
    }

    #[tokio::test]
    async fn resume_prompt_is_shorter_than_fresh() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Resume prompt test".into(),
                body: Some("A long body describing the task in detail.".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fresh_prompt = build_issue_prompt(&db, &issue, false).await.unwrap();
        let resume_prompt = build_issue_prompt(&db, &issue, true).await.unwrap();

        assert!(
            resume_prompt.len() < fresh_prompt.len(),
            "resume ({}) should be shorter than fresh ({})",
            resume_prompt.len(),
            fresh_prompt.len(),
        );
        assert!(resume_prompt.contains("Continuing work on"));
        assert!(!resume_prompt.contains(&issue.body));
    }

    #[tokio::test]
    async fn resume_prompt_includes_revision_feedback() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Revision resume test".into(),
                body: Some("Original task".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Add a result comment (marks this as a revision)
        issue::add_comment(&db, &issue.id, "agent", "First attempt done", Some("result")).await.unwrap();
        // Add revision feedback
        issue::add_comment(&db, &issue.id, "user", "Fix the tests", Some("revision")).await.unwrap();

        let prompt = build_issue_prompt(&db, &issue, true).await.unwrap();
        assert!(prompt.contains("New comments since last run"));
        assert!(prompt.contains("Fix the tests"));
        assert!(prompt.contains("Address the revision feedback"));
    }

    #[tokio::test]
    async fn resume_prompt_includes_retry_error() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "Retry resume test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Set num_tries > 0 and error (marks as retry, not revision since no result comment)
        issue::update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                num_tries: Some(1),
                error: Some("compilation failed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let issue = issue::get_issue(&db, &issue.id).await.unwrap();
        let prompt = build_issue_prompt(&db, &issue, true).await.unwrap();
        assert!(prompt.contains("compilation failed"));
        assert!(prompt.contains("partial changes"));
    }
}
