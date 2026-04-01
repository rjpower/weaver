use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use clap::{CommandFactory, Parser};
use serde::Serialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use weaver::{
    add_comment, create_issue, get_comments, get_issue, get_issue_tree, list_issues,
    update_issue, AgentRunner, Comment, CreateIssueParams, Executor, ExecutorConfig, Issue,
    IssueStatus, ListFilter, NotifyHooks, UpdateIssueParams,
};
use weaver::issue::IssueScope;

#[derive(Serialize)]
struct IssueWithComments {
    #[serde(flatten)]
    issue: Issue,
    comments: Vec<Comment>,
}

#[derive(Parser)]
#[command(name = "weaver", about = "Task management & DAG execution engine")]
struct Cli {
    /// Path to the database file (default: .weaver/db.sqlite)
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    /// Output JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Returns true if output should be JSON (explicit --json flag or non-TTY stdout).
fn use_json(cli_flag: bool) -> bool {
    cli_flag || !std::io::stdout().is_terminal()
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Manage issues
    Issue {
        #[command(subcommand)]
        command: IssueCommands,
    },
    /// Manage skills (workflow templates)
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Manage workflows
    #[command(hide = true)]
    Workflow {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Manage git worktrees
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommands,
    },
    /// Start the HTTP API server with executor
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
        /// Webhook URL for notifications (Slack/Discord compatible)
        #[arg(long, env = "WEAVER_WEBHOOK_URL")]
        webhook_url: Option<String>,
    },
    /// Manage configuration settings
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[derive(clap::Subcommand)]
enum IssueCommands {
    /// Create a new issue
    Create {
        /// Issue title
        title: String,
        /// Description text or @file path
        #[arg(long)]
        body: Option<String>,
        /// Issue IDs this depends on (comma-separated)
        #[arg(long, value_delimiter = ',')]
        depends_on: Vec<String>,
        /// Tags for categorization (repeatable)
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long, default_value = "0")]
        priority: i32,
        /// JSON context string or @file path
        #[arg(long)]
        context: Option<String>,
        /// Maximum retry attempts
        #[arg(long)]
        max_tries: Option<i32>,
        /// Parent issue ID
        #[arg(long)]
        parent: Option<String>,
        /// Share the parent's worktree instead of auto-creating a new one
        #[arg(long)]
        same_worktree: bool,
        /// Start the executor and run the issue until completion
        #[arg(long)]
        run: bool,
        /// Open $EDITOR to compose the issue body before submitting
        #[arg(long, short)]
        edit: bool,
        /// Sandbox level: readonly, default_dev, unrestricted
        #[arg(long)]
        sandbox: Option<String>,
    },
    /// List issues
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, default_value = "20")]
        limit: i64,
    },
    /// Show issue details
    Show { id: String },
    /// Update an issue
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        status: Option<String>,
        /// Replace tags (repeatable)
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Cancel a running or pending issue
    Cancel { id: String },
    /// Reset stuck running issues back to pending
    Reset { id: Option<String> },
    /// Add a comment to an issue
    Comment {
        /// Issue ID
        id: String,
        /// Comment body
        body: String,
    },
    /// Request a revision of a completed/failed issue with feedback
    Revise {
        /// Issue ID
        id: String,
        /// Feedback text for the revision
        feedback: String,
        /// Replace tags (repeatable)
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Request human review for the current issue (used by agents)
    ReviewRequest {
        /// Issue ID
        id: String,
        /// Summary of what to review
        #[arg(long)]
        summary: Option<String>,
    },
    /// Approve an issue that is awaiting review
    Approve {
        /// Issue ID
        id: String,
        /// Optional approval comment
        #[arg(long)]
        comment: Option<String>,
    },
    /// Print the worktree path for an issue (for quick cd)
    Open {
        /// Issue ID
        id: String,
    },
    /// Wait for an issue to reach a terminal state
    Wait {
        id: String,
        /// Timeout in seconds (0 = no timeout)
        #[arg(long, default_value = "0")]
        timeout: u64,
    },
    /// Wait for any of the given issues to reach a terminal state
    WaitAny {
        ids: Vec<String>,
        /// Timeout in seconds (0 = no timeout)
        #[arg(long, default_value = "0")]
        timeout: u64,
    },
    /// Wait for all of the given issues to reach terminal states
    WaitAll {
        ids: Vec<String>,
        /// Timeout in seconds (0 = no timeout)
        #[arg(long, default_value = "0")]
        timeout: u64,
    },
    /// Show issue hierarchy as a tree
    Tree {
        /// Root issue ID
        id: String,
    },
}

#[derive(clap::Subcommand)]
enum SkillCommands {
    /// List available skills
    #[command(alias = "ls")]
    List,
    /// Execute issues
    Run {
        /// Specific issue ID to run
        id: Option<String>,
        /// Run in continuous watch mode
        #[arg(long)]
        watch: bool,
        /// Run the most recently created pending issue
        #[arg(long)]
        latest: bool,
        /// Webhook URL for notifications (Slack/Discord compatible)
        #[arg(long, env = "WEAVER_WEBHOOK_URL")]
        webhook_url: Option<String>,
    },
}

#[derive(clap::Subcommand)]
enum WorktreeCommands {
    /// Create a git worktree for isolated work
    Create {
        branch: String,
        #[arg(long, default_value = "main")]
        base: String,
        /// Issue ID to attach the worktree to (defaults to WEAVER_ISSUE_ID env var)
        #[arg(long)]
        issue: Option<String>,
    },
    /// Attach an existing worktree directory to an issue
    Attach {
        /// Issue ID
        issue: String,
        /// Path to the worktree directory (defaults to cwd)
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Merge issue branch(es) into the current branch
    Merge {
        /// Issue IDs whose branches to merge
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Garbage collect stale worktrees, keeping active and the last N
    Gc {
        /// Number of recent terminal worktrees to keep (overrides worktree.keep_count setting)
        #[arg(long)]
        keep: Option<usize>,
    },
}

#[derive(clap::Subcommand)]
enum ConfigCommands {
    /// Set a configuration value
    Set { key: String, value: String },
    /// Get a configuration value
    Get { key: String },
    /// List all configuration values
    #[command(alias = "ls")]
    List,
    /// Delete a configuration value
    Delete { key: String },
}

/// Open $EDITOR with a template containing the title and body, parse the result.
/// The format is: first line = title, blank line separator, rest = body.
/// Lines starting with # are comments and are stripped.
fn edit_issue_interactively(
    title: &str,
    body: Option<&str>,
) -> anyhow::Result<(String, Option<String>)> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string());

    let template = format!(
        "{title}\n\n{body}\n# Edit the issue above. First line is the title.\n# Everything after the first blank line is the body.\n# Lines starting with # are removed.\n# Save and close the editor to create the issue.\n# Leave the title empty to abort.",
        title = title,
        body = body.unwrap_or(""),
    );

    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("weaver-issue-{}.md", std::process::id()));
    std::fs::write(&tmp_path, &template)?;

    let status = std::process::Command::new(&editor)
        .arg(&tmp_path)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to launch editor '{editor}': {e}"))?;

    if !status.success() {
        std::fs::remove_file(&tmp_path).ok();
        anyhow::bail!("editor exited with non-zero status");
    }

    let content = std::fs::read_to_string(&tmp_path)?;
    std::fs::remove_file(&tmp_path).ok();

    // Strip comment lines
    let lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect();

    // First non-empty line is the title
    let title = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_default();

    if title.is_empty() {
        anyhow::bail!("aborted: empty title");
    }

    // Find the first blank line after the title, everything after is body
    let title_idx = lines.iter().position(|l| l.trim() == title.as_str()).unwrap_or(0);
    let after_title = &lines[title_idx + 1..];

    // Skip leading blank lines to find body start
    let body_start = after_title.iter().position(|l| !l.trim().is_empty());
    let body = body_start.map(|idx| {
        after_title[idx..]
            .join("\n")
            .trim_end()
            .to_string()
    });
    let body = body.filter(|b| !b.is_empty());

    Ok((title, body))
}

fn resolve_source(value: &str) -> anyhow::Result<String> {
    if let Some(path) = value.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read {path}: {e}"))
    } else {
        Ok(value.to_string())
    }
}

fn resolve_context(value: &str) -> anyhow::Result<serde_json::Value> {
    let text = if let Some(path) = value.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read {path}: {e}"))?
    } else {
        value.to_string()
    };
    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("invalid JSON context: {e}"))
}

fn print_wait_summary(issue: &Issue, comments: &[Comment]) {
    match issue.status {
        IssueStatus::Completed => println!("Task {} \"{}\" completed.", issue.id, issue.title),
        IssueStatus::Failed => {
            let tries_info = if issue.max_tries > 1 {
                format!(" (attempt {}/{})", issue.num_tries, issue.max_tries)
            } else {
                String::new()
            };
            println!("Task {} \"{}\" failed{tries_info}.", issue.id, issue.title);
        }
        ref other => println!("Task {} \"{}\" {}.", issue.id, issue.title, other),
    }
    println!();

    // Result comment — full, not truncated (this is the deliverable)
    if let Some(result) = comments
        .iter()
        .rev()
        .find(|c| c.tag.as_deref() == Some("result"))
    {
        println!("Result:");
        for line in result.body.lines() {
            println!("  {line}");
        }
        println!();
    }

    if let Some(ref error) = issue.error {
        println!("Error: {error}");
        println!();
    }

    // Branch + worktree + diff stat
    if let Some(work_dir) = issue.context.get("work_dir").and_then(|v| v.as_str()) {
        if let Ok(branch_output) = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(work_dir)
            .output()
        {
            if branch_output.status.success() {
                let branch = String::from_utf8_lossy(&branch_output.stdout)
                    .trim()
                    .to_string();
                println!("Branch: {branch}");
            }
        }
        println!("Worktree: {work_dir}");

        if let Ok(output) = std::process::Command::new("git")
            .args(["diff", "--stat", "main..HEAD"])
            .current_dir(work_dir)
            .output()
        {
            if output.status.success() {
                let stat = String::from_utf8_lossy(&output.stdout);
                if !stat.trim().is_empty() {
                    for line in stat.lines() {
                        println!("  {line}");
                    }
                }
            }
        }
        println!();
    }

    // Actionable commands
    if issue.status == IssueStatus::Completed {
        println!("To merge into your branch:");
        println!("  weaver worktree merge {}", issue.id);
    } else if issue.status == IssueStatus::Failed {
        println!("To retry:");
        println!("  weaver issue reset {}", issue.id);
    }

    // Deeper inspection (secondary)
    let progress_count = comments.iter().filter(|c| c.tag.is_none()).count();
    println!("\nTo inspect further:");
    if progress_count > 0 {
        println!("  weaver issue show {}    # {progress_count} progress comments", issue.id);
    } else {
        println!("  weaver issue show {}", issue.id);
    }
    if let Some(work_dir) = issue.context.get("work_dir").and_then(|v| v.as_str()) {
        println!("  git -C {work_dir} diff main..HEAD");
    }
}

fn default_runner(api_url: String) -> Arc<AgentRunner> {
    let skills_dir = weaver::find_skills_root().unwrap_or_else(|e| {
        tracing::warn!("Skill discovery failed: {e}; built-in skills will not be available");
        PathBuf::from("/nonexistent")
    });
    Arc::new(AgentRunner {
        api_url,
        workflows_dir: skills_dir,
        sdk_dir: PathBuf::from("/nonexistent"),
        binary: std::env::var("WEAVER_AGENT_BINARY").unwrap_or_else(|_| "claude".into()),
    })
}

async fn start_ephemeral_server(
    db: weaver::Db,
) -> anyhow::Result<(String, CancellationToken)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    tokio::spawn(async move {
        if let Err(e) = weaver::serve(db, listener, cancel_clone).await {
            tracing::error!(error = %e, "Ephemeral server failed");
        }
    });

    Ok((format!("http://{addr}"), cancel))
}

/// Polls the DB until the issue reaches a terminal state, returning the issue.
async fn poll_until_terminal(
    db: &weaver::Db,
    id: &str,
) -> anyhow::Result<weaver::Issue> {
    loop {
        let issue = get_issue(db, id).await?;
        if issue.status.is_terminal() {
            return Ok(issue);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Wraps a future with an optional timeout. timeout_secs == 0 means no timeout.
async fn with_timeout<T>(
    timeout_secs: u64,
    fut: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    if timeout_secs == 0 {
        fut.await
    } else {
        tokio::time::timeout(Duration::from_secs(timeout_secs), fut)
            .await
            .map_err(|_| anyhow::anyhow!("timed out after {timeout_secs}s"))?
    }
}

/// Sanitizes a branch name for use as a directory name.
fn safe_branch_name(branch: &str) -> String {
    branch.replace('/', "-")
}

#[derive(Serialize)]
struct MergeResult {
    merged: Vec<String>,
    conflicts: Vec<String>,
}

fn status_icon(status: &IssueStatus) -> &'static str {
    match status {
        IssueStatus::Completed => "✓",
        IssueStatus::Failed => "✗",
        IssueStatus::Running => "◷",
        IssueStatus::Pending => "○",
        IssueStatus::Blocked => "⊘",
        IssueStatus::AwaitingReview => "⏸",
        IssueStatus::ValidationFailed => "!",
    }
}

fn format_duration(created_at: &str, completed_at: Option<&str>) -> String {
    let created = chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(created_at).map(|dt| dt.naive_utc()));
    let end = match completed_at {
        Some(s) => chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| chrono::DateTime::parse_from_rfc3339(s).map(|dt| dt.naive_utc())),
        None => Ok(Utc::now().naive_utc()),
    };

    let (Ok(start), Ok(end)) = (created, end) else {
        return String::new();
    };

    let secs = (end - start).num_seconds().max(0);
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let remaining_secs = secs % 60;

    if hours > 0 {
        format!("{hours}h {mins}m")
    } else if mins > 0 {
        format!("{mins}m {remaining_secs}s")
    } else {
        format!("{remaining_secs}s")
    }
}

fn format_tree_line(issue: &Issue) -> String {
    let icon = status_icon(&issue.status);
    let duration = format_duration(&issue.created_at, issue.completed_at.as_deref());

    let mut parts = vec![issue.id.clone()];
    parts.push(icon.to_string());
    parts.push(issue.title.clone());
    if !duration.is_empty() {
        parts.push(format!("({duration})"));
    }

    parts.join(" ")
}

fn print_issue_tree(root: &Issue, descendants: &[Issue]) {
    let mut children_map: HashMap<&str, Vec<&Issue>> = HashMap::new();
    for issue in descendants {
        if let Some(ref parent_id) = issue.parent_issue_id {
            children_map.entry(parent_id.as_str()).or_default().push(issue);
        }
    }
    // children_map values are already sorted by created_at since get_issue_tree
    // fetches via list_issues which orders by created_at

    println!("{}", format_tree_line(root));
    print_children(&children_map, &root.id, "");
}

fn print_children(children_map: &HashMap<&str, Vec<&Issue>>, parent_id: &str, prefix: &str) {
    let Some(children) = children_map.get(parent_id) else {
        return;
    };
    let last_idx = children.len() - 1;
    for (i, child) in children.iter().enumerate() {
        let is_last = i == last_idx;
        let connector = if is_last { "└── " } else { "├── " };
        println!("{prefix}{connector}{}", format_tree_line(child));

        let child_prefix = if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}│   ")
        };
        print_children(children_map, &child.id, &child_prefix);
    }
}

fn build_tree_json(root: &Issue, descendants: &[Issue]) -> Value {
    let mut children_map: HashMap<&str, Vec<&Issue>> = HashMap::new();
    for issue in descendants {
        if let Some(ref parent_id) = issue.parent_issue_id {
            children_map.entry(parent_id.as_str()).or_default().push(issue);
        }
    }
    build_tree_node_json(root, &children_map)
}

fn build_tree_node_json(issue: &Issue, children_map: &HashMap<&str, Vec<&Issue>>) -> Value {
    let children: Vec<Value> = children_map
        .get(issue.id.as_str())
        .map(|kids| kids.iter().map(|c| build_tree_node_json(c, children_map)).collect())
        .unwrap_or_default();

    serde_json::json!({
        "id": issue.id,
        "title": issue.title,
        "status": issue.status,
        "tags": issue.tags,
        "duration": format_duration(&issue.created_at, issue.completed_at.as_deref()),
        "created_at": issue.created_at,
        "completed_at": issue.completed_at,
        "children": children,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "weaver=info".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();
    let explicit_json = cli.json;
    let json = use_json(cli.json);
    let db_path = cli.db.unwrap_or_else(weaver::db::default_db_path);
    let db = weaver::db::connect(&db_path).await?;

    match cli.command {
        Commands::Issue { command } => match command {
            IssueCommands::Create {
                title,
                body,
                depends_on,
                tag,
                priority,
                context,
                max_tries,
                parent,
                same_worktree,
                run,
                edit,
                sandbox,
            } => {
                let body = body.map(|b| resolve_source(&b)).transpose()?;
                let (title, body) = if edit {
                    edit_issue_interactively(&title, body.as_deref())?
                } else {
                    (title, body)
                };
                let mut ctx = context.map(|c| resolve_context(&c)).transpose()?;

                if let Some(ref level) = sandbox {
                    level.parse::<weaver::SandboxLevel>()?;
                    let obj = ctx.get_or_insert_with(|| Value::Object(Default::default()));
                    obj.as_object_mut()
                        .expect("context must be a JSON object")
                        .insert("sandbox".into(), serde_json::json!(level));
                }

                if same_worktree {
                    let obj = ctx.get_or_insert_with(|| Value::Object(Default::default()));
                    obj.as_object_mut()
                        .expect("context must be a JSON object")
                        .insert("same_worktree".into(), serde_json::json!(true));
                }

                let issue = create_issue(
                    &db,
                    CreateIssueParams {
                        title,
                        body,
                        context: ctx,
                        dependencies: depends_on,
                        tags: tag,
                        priority,
                        max_tries,
                        parent_issue_id: parent,
                        ..Default::default()
                    },
                )
                .await?;

                if explicit_json {
                    println!("{}", serde_json::to_string_pretty(&issue)?);
                } else {
                    println!("{}", issue.id);
                }

                if run {
                    let (api_url, server_cancel) = start_ephemeral_server(db.clone()).await?;
                    eprintln!("API server: {api_url}");
                    let runner = default_runner(api_url.clone());
                    let hooks: Arc<dyn weaver::ExecutorHooks> =
                        Arc::new(NotifyHooks::new(db.clone()));
                    let loop_cancel = CancellationToken::new();
                    let loop_cancel_clone = loop_cancel.clone();
                    let loop_executor = Executor::with_hooks(
                        db.clone(),
                        ExecutorConfig::default(),
                        default_runner(api_url),
                        hooks.clone(),
                    );
                    tokio::spawn(async move {
                        if let Err(e) = loop_executor.run_loop(loop_cancel_clone).await {
                            tracing::error!(error = %e, "Executor loop failed");
                        }
                    });

                    let wait_executor = Executor::with_hooks(
                        db.clone(),
                        ExecutorConfig::default(),
                        runner,
                        hooks,
                    );
                    let finished = wait_executor
                        .wait_for_issue(&issue.id, CancellationToken::new())
                        .await?;
                    loop_cancel.cancel();
                    server_cancel.cancel();

                    let exit_code = if finished.status == IssueStatus::Completed { 0 } else { 1 };
                    eprintln!("Issue {} finished with status: {}", finished.id, finished.status);
                    std::process::exit(exit_code);
                }
            }

            IssueCommands::List { status, tag, limit } => {
                let status_filter = status
                    .map(|s| s.parse::<IssueStatus>())
                    .transpose()?;

                let issues = list_issues(
                    &db,
                    ListFilter {
                        status: status_filter,
                        tag,
                        limit: Some(limit),
                        ..Default::default()
                    },
                )
                .await?
                .issues;

                if json {
                    println!("{}", serde_json::to_string_pretty(&issues)?);
                    return Ok(());
                }

                if issues.is_empty() {
                    println!("No issues found.");
                    return Ok(());
                }

                let parent_ids: std::collections::HashSet<&str> = issues
                    .iter()
                    .filter(|i| i.parent_issue_id.is_none())
                    .map(|i| i.id.as_str())
                    .collect();

                let mut ordered: Vec<(&weaver::Issue, bool)> = Vec::new();
                for issue in &issues {
                    if issue.parent_issue_id.is_none() {
                        ordered.push((issue, false));
                        for child in &issues {
                            if child.parent_issue_id.as_deref() == Some(&issue.id) {
                                ordered.push((child, true));
                            }
                        }
                    }
                }
                for issue in &issues {
                    if let Some(ref pid) = issue.parent_issue_id {
                        if !parent_ids.contains(pid.as_str()) {
                            ordered.push((issue, true));
                        }
                    }
                }

                for (issue, is_child) in ordered {
                    let prefix = if is_child { "  └ " } else { "" };
                    let tags = if issue.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", issue.tags.join(", "))
                    };

                    println!(
                        "{}  {:<12} {prefix}{title}{tags}",
                        issue.id,
                        issue.status.to_string(),
                        title = issue.title,
                    );
                }
            }

            IssueCommands::Show { id } => {

                let issue = get_issue(&db, &id).await?;

                if json {
                    println!("{}", serde_json::to_string_pretty(&issue)?);
                    return Ok(());
                }

                println!("ID:         {}", issue.id);
                println!("Title:      {}", issue.title);
                println!("Status:     {}", issue.status);
                println!("Priority:   {}", issue.priority);
                println!("Tries:      {}/{}", issue.num_tries, issue.max_tries);
                println!("Created:    {}", issue.created_at);
                println!("Updated:    {}", issue.updated_at);
                if let Some(ref completed) = issue.completed_at {
                    println!("Completed:  {completed}");
                }
                if !issue.body.is_empty() {
                    println!("\nBody:\n{}", issue.body);
                }
                if !issue.dependencies.is_empty() {
                    println!("\nDependencies: {}", issue.dependencies.join(", "));
                }
                if !issue.tags.is_empty() {
                    println!("Tags: {}", issue.tags.join(", "));
                }
                if let Some(ref result) = weaver::get_result_comment(&db, &id).await? {
                    println!("\nResult:\n{result}");
                }
                if let Some(ref error) = issue.error {
                    println!("\nError:\n{error}");
                }

                let comments = get_comments(&db, &id).await?;
                if !comments.is_empty() {
                    println!("\n--- Comments ---");
                    for c in comments {
                        println!("[{}] {} ({})", c.created_at, c.body, c.author);
                    }
                }
            }

            IssueCommands::Update {
                id,
                title,
                body,
                status,
                tag,
            } => {

                let status_val = status
                    .map(|s| s.parse::<IssueStatus>())
                    .transpose()?;
                let tags = if tag.is_empty() { None } else { Some(tag) };

                let updated = update_issue(
                    &db,
                    &id,
                    UpdateIssueParams {
                        title,
                        body,
                        status: status_val,
                        tags,
                        ..Default::default()
                    },
                )
                .await?;

                println!("Updated issue {} (status: {})", updated.id, updated.status);
            }

            IssueCommands::Cancel { id } => {

                let issue = get_issue(&db, &id).await?;
                if issue.status.is_terminal() {
                    anyhow::bail!(
                        "issue {} is already in terminal state: {}",
                        id,
                        issue.status
                    );
                }

                update_issue(
                    &db,
                    &id,
                    UpdateIssueParams {
                        status: Some(IssueStatus::Failed),
                        error: Some("Cancelled by user".into()),
                        ..Default::default()
                    },
                )
                .await?;
                println!("Cancelled issue {id}");

                let children = list_issues(
                    &db,
                    ListFilter {
                        scope: IssueScope::ChildrenOf(id.clone()),
                        ..Default::default()
                    },
                )
                .await?
                .issues;
                let mut cancelled_children = 0;
                for child in children {
                    if child.status.is_terminal() {
                        continue;
                    }
                    update_issue(
                        &db,
                        &child.id,
                        UpdateIssueParams {
                            status: Some(IssueStatus::Failed),
                            error: Some("Parent cancelled by user".into()),
                            ..Default::default()
                        },
                    )
                    .await?;
                    cancelled_children += 1;
                }
                if cancelled_children > 0 {
                    println!("Cancelled {cancelled_children} child issue(s)");
                }
            }

            IssueCommands::Reset { id } => {
                if let Some(id) = id {
    
                    let issue = get_issue(&db, &id).await?;
                    if issue.status.is_terminal() || issue.status == IssueStatus::Running {
                        update_issue(
                            &db,
                            &id,
                            UpdateIssueParams {
                                status: Some(IssueStatus::Pending),
                                ..Default::default()
                            },
                        )
                        .await?;
                        println!("Reset issue {id} to pending (was {})", issue.status);
                    } else {
                        anyhow::bail!(
                            "issue {id} is already pending or blocked (status: {})",
                            issue.status
                        );
                    }
                } else {
                    let running = list_issues(
                        &db,
                        ListFilter {
                            status: Some(IssueStatus::Running),
                            ..Default::default()
                        },
                    )
                    .await?
                    .issues;
                    if running.is_empty() {
                        println!("No running issues to reset.");
                        return Ok(());
                    }
                    for issue in &running {
                        update_issue(
                            &db,
                            &issue.id,
                            UpdateIssueParams {
                                status: Some(IssueStatus::Pending),
                                ..Default::default()
                            },
                        )
                        .await?;
                    }
                    println!("Reset {} running issue(s) to pending", running.len());
                }
            }

            IssueCommands::Comment { id, body } => {

                let author = if std::env::var("WEAVER_ISSUE_ID").is_ok() { "agent" } else { "user" };
                add_comment(&db, &id, author, &body, None).await?;
                println!("Comment added to issue {id}");
            }

            IssueCommands::Revise { id, feedback, tag } => {

                let issue = get_issue(&db, &id).await?;
                if !issue.status.is_terminal() && issue.status != IssueStatus::AwaitingReview {
                    anyhow::bail!(
                        "issue {id} cannot be revised (status: {}). Only completed/failed/validation_failed/awaiting_review issues can be revised.",
                        issue.status
                    );
                }

                add_comment(&db, &id, "revision", &feedback, Some("revision")).await?;
                let tags = if tag.is_empty() { None } else { Some(tag) };
                update_issue(
                    &db,
                    &id,
                    UpdateIssueParams {
                        status: Some(IssueStatus::Pending),
                        error: Some(String::new()),
                        tags,
                        ..Default::default()
                    },
                )
                .await?;
                println!("Revision requested for issue {id}");
            }

            IssueCommands::ReviewRequest { id, summary } => {

                let issue = get_issue(&db, &id).await?;
                if issue.status != IssueStatus::Running {
                    anyhow::bail!(
                        "issue {id} is not running (status: {})",
                        issue.status
                    );
                }
                if let Some(ref s) = summary {
                    add_comment(&db, &id, "review-request", s, None).await?;
                }
                update_issue(
                    &db,
                    &id,
                    UpdateIssueParams {
                        status: Some(IssueStatus::AwaitingReview),
                        ..Default::default()
                    },
                )
                .await?;
                if json {
                    let issue = get_issue(&db, &id).await?;
                    println!("{}", serde_json::to_string_pretty(&issue)?);
                } else {
                    println!("Review requested for issue {id}");
                }
            }

            IssueCommands::Approve { id, comment } => {

                let issue = get_issue(&db, &id).await?;
                if issue.status != IssueStatus::AwaitingReview {
                    anyhow::bail!(
                        "issue {id} is not awaiting review (status: {})",
                        issue.status
                    );
                }
                if let Some(ref c) = comment {
                    add_comment(&db, &id, "reviewer", c, None).await?;
                }
                update_issue(
                    &db,
                    &id,
                    UpdateIssueParams {
                        status: Some(IssueStatus::Completed),
                        ..Default::default()
                    },
                )
                .await?;
                if json {
                    let issue = get_issue(&db, &id).await?;
                    println!("{}", serde_json::to_string_pretty(&issue)?);
                } else {
                    println!("Approved issue {id}");
                }
            }

            IssueCommands::Open { id } => {

                let issue = get_issue(&db, &id).await?;
                match issue.context.get("work_dir").and_then(|v| v.as_str()) {
                    Some(wd) => {
                        if std::path::Path::new(wd).exists() {
                            println!("{wd}");
                        } else {
                            anyhow::bail!("worktree directory does not exist: {wd}");
                        }
                    }
                    None => anyhow::bail!("issue {id} has no work_dir in context"),
                }
            }

            IssueCommands::Wait { id, timeout } => {

                if let Ok(caller_id) = std::env::var("WEAVER_ISSUE_ID") {
                    let target = get_issue(&db, &id).await?;
                    add_comment(&db, &caller_id, "weaver",
                        &format!("Waiting for {id} ({})", target.title),
                        Some("generated")).await.ok();
                }
                let issue = with_timeout(timeout, poll_until_terminal(&db, &id)).await?;
                let comments = get_comments(&db, &issue.id).await?;
                if json {
                    let output = IssueWithComments { issue, comments };
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    print_wait_summary(&issue, &comments);
                }
            }

            IssueCommands::WaitAny { ids, timeout } => {
                if ids.is_empty() {
                    anyhow::bail!("wait-any requires at least one issue ID");
                }
                if let Ok(caller_id) = std::env::var("WEAVER_ISSUE_ID") {
                    add_comment(&db, &caller_id, "weaver",
                        &format!("Waiting for any of {} issues: {}", ids.len(), ids.join(", ")),
                        Some("generated")).await.ok();
                }

                let issue = with_timeout(timeout, async {
                    loop {
                        for id in &ids {
                            let issue = get_issue(&db, id).await?;
                            if issue.status.is_terminal() {
                                return Ok(issue);
                            }
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                })
                .await?;

                let comments = get_comments(&db, &issue.id).await?;
                if json {
                    let output = IssueWithComments { issue, comments };
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    print_wait_summary(&issue, &comments);
                }
            }

            IssueCommands::WaitAll { ids, timeout } => {
                if ids.is_empty() {
                    anyhow::bail!("wait-all requires at least one issue ID");
                }

                if let Ok(caller_id) = std::env::var("WEAVER_ISSUE_ID") {
                    add_comment(&db, &caller_id, "weaver",
                        &format!("Waiting for {} issues: {}", ids.len(), ids.join(", ")),
                        Some("generated")).await.ok();
                }

                let issues = with_timeout(timeout, async {
                    loop {
                        let mut all_terminal = true;
                        let mut results = Vec::with_capacity(ids.len());
                        for id in &ids {
                            let issue = get_issue(&db, id).await?;
                            if !issue.status.is_terminal() {
                                all_terminal = false;
                            }
                            results.push(issue);
                        }
                        if all_terminal {
                            return Ok(results);
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                })
                .await?;

                if json {
                    let mut output = Vec::new();
                    for issue in issues {
                        let comments = get_comments(&db, &issue.id).await?;
                        output.push(IssueWithComments { issue, comments });
                    }
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    let total = issues.len();
                    let mut completed_ids = Vec::new();
                    let mut failed_ids = Vec::new();

                    for (i, issue) in issues.iter().enumerate() {
                        println!("--- Task {}/{total} ---", i + 1);
                        let comments = get_comments(&db, &issue.id).await?;
                        print_wait_summary(issue, &comments);
                        println!();

                        match issue.status {
                            IssueStatus::Completed => completed_ids.push(issue.id.clone()),
                            IssueStatus::Failed => failed_ids.push(issue.id.clone()),
                            _ => {}
                        }
                    }

                    println!("---");
                    println!("Summary: {} completed, {} failed.", completed_ids.len(), failed_ids.len());
                    if !completed_ids.is_empty() {
                        println!();
                        println!("To merge completed branches into your branch:");
                        println!("  weaver worktree merge {}", completed_ids.join(" "));
                    }
                    if !failed_ids.is_empty() {
                        println!();
                        println!("To retry failed tasks:");
                        for s in &failed_ids {
                            println!("  weaver issue reset {s}");
                        }
                    }
                }
            }

            IssueCommands::Tree { id } => {
                let issue = get_issue(&db, &id).await?;
                let descendants = get_issue_tree(&db, &id).await?;

                if json {
                    let tree_json = build_tree_json(&issue, &descendants);
                    println!("{}", serde_json::to_string_pretty(&tree_json)?);
                } else {
                    print_issue_tree(&issue, &descendants);
                }
            }
        },

        Commands::Skill { command } | Commands::Workflow { command } => match command {
            SkillCommands::List => {
                let cwd = std::env::current_dir().unwrap_or_default();
                let builtins_dir = weaver::find_skills_root()
                    .unwrap_or_else(|_| PathBuf::from("/nonexistent"));
                let skills = weaver::list_skills(&builtins_dir, &cwd);

                if skills.is_empty() {
                    println!("No skills found.");
                    return Ok(());
                }

                println!("{:<20} {:<10} {}", "TAG", "SOURCE", "PATH");
                println!("{}", "-".repeat(70));
                for s in &skills {
                    println!("{:<20} {:<10} {}", s.tag, s.source, s.path.display());
                }
            }

            SkillCommands::Run {
                id,
                watch,
                latest,
                webhook_url,
            } => {
                if let Some(url) = webhook_url {
                    weaver::settings::set(&db, "notify.generic.url", &url).await.ok();
                }
                let (api_url, server_cancel) = start_ephemeral_server(db.clone()).await?;
                println!("API server: {api_url}");
                let runner = default_runner(api_url.clone());
                let hooks: Arc<dyn weaver::ExecutorHooks> =
                    Arc::new(NotifyHooks::new(db.clone()));
                let executor = Executor::with_hooks(
                    db.clone(),
                    ExecutorConfig::default(),
                    runner,
                    hooks.clone(),
                );

                let wait_id = if let Some(id) = id {
                    Some(id)
                } else if latest {
                    sqlx::query_as::<_, (String,)>(
                        "SELECT id FROM issues WHERE status = 'pending' ORDER BY created_at DESC LIMIT 1",
                    )
                    .fetch_optional(&db)
                    .await?
                    .map(|(id,)| id)
                } else {
                    None
                };

                if latest && wait_id.is_none() {
                    println!("No pending issues found.");
                    server_cancel.cancel();
                    return Ok(());
                }

                let result = async {
                    if let Some(ref wait_id) = wait_id {
                        let loop_cancel = CancellationToken::new();
                        let loop_cancel_clone = loop_cancel.clone();
                        let loop_executor = Executor::with_hooks(
                            db.clone(),
                            ExecutorConfig::default(),
                            default_runner(api_url),
                            hooks,
                        );
                        tokio::spawn(async move {
                            if let Err(e) = loop_executor.run_loop(loop_cancel_clone).await {
                                tracing::error!(error = %e, "Executor loop failed");
                            }
                        });

                        let issue = executor
                            .wait_for_issue(wait_id, CancellationToken::new())
                            .await?;
                        loop_cancel.cancel();
                        println!("Issue {} finished with status: {}", issue.id, issue.status);
                    } else if watch {
                        tracing::info!("Executor watching for issues...");
                        let cancel = CancellationToken::new();
                        let cancel_clone = cancel.clone();
                        tokio::spawn(async move {
                            tokio::signal::ctrl_c().await.ok();
                            cancel_clone.cancel();
                        });
                        executor.run_loop(cancel).await?;
                    } else {
                        let report = executor.run_once().await?;
                        println!(
                            "Executed {} issues: {} completed, {} failed",
                            report.executed.len(),
                            report.completed.len(),
                            report.failed.len()
                        );
                    }
                    Ok::<(), anyhow::Error>(())
                }
                .await;

                server_cancel.cancel();
                result?;
            }
        },

        Commands::Worktree { command } => match command {
            WorktreeCommands::Create { branch, base, issue } => {
                let safe_name = safe_branch_name(&branch);
                let worktree_dir = PathBuf::from(".weaver/worktrees");
                std::fs::create_dir_all(&worktree_dir)?;
                let worktree_path = worktree_dir.join(&safe_name);

                // Clean up stale worktree references first
                tokio::process::Command::new("git")
                    .args(["worktree", "prune"])
                    .status()
                    .await?;

                // Fetch origin so branches are fresh (ignore failures for offline/no-remote)
                tokio::process::Command::new("git")
                    .args(["fetch", "origin", &base, "--no-tags", "--quiet"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await
                    .ok();

                let status = tokio::process::Command::new("git")
                    .args([
                        "worktree",
                        "add",
                        "-b",
                        &branch,
                        worktree_path.to_str().unwrap(),
                        &base,
                    ])
                    .status()
                    .await?;

                // If the branch already exists, try adding without -b
                if !status.success() {
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
                        anyhow::bail!("failed to create worktree for branch '{branch}'");
                    }
                }

                let abs_path = std::fs::canonicalize(&worktree_path)?;

                // Attach worktree to issue (explicit --issue flag or WEAVER_ISSUE_ID env var)
                let attach_issue_id = issue.or_else(|| std::env::var("WEAVER_ISSUE_ID").ok());
                if let Some(issue_id) = attach_issue_id {
                    let current = get_issue(&db, &issue_id).await?;
                    let mut ctx = current.context.as_object().cloned().unwrap_or_default();
                    ctx.insert("work_dir".into(), serde_json::json!(abs_path.to_string_lossy()));
                    ctx.insert("branch".into(), serde_json::json!(branch));
                    ctx.insert("base_branch".into(), serde_json::json!(base));
                    update_issue(
                        &db,
                        &issue_id,
                        UpdateIssueParams {
                            context: Some(Value::Object(ctx)),
                            ..Default::default()
                        },
                    )
                    .await?;
                }

                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&abs_path.to_string_lossy())?
                    );
                } else {
                    println!("{}", abs_path.display());
                }
            }

            WorktreeCommands::Attach { issue, path } => {
                let issue_id = issue;
                let dir = path.unwrap_or_else(|| std::env::current_dir().unwrap());
                let abs_path = std::fs::canonicalize(&dir)?;

                // Detect branch from the worktree's HEAD
                let branch_output = tokio::process::Command::new("git")
                    .args(["rev-parse", "--abbrev-ref", "HEAD"])
                    .current_dir(&abs_path)
                    .output()
                    .await?;
                let branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();

                let current = get_issue(&db, &issue_id).await?;
                let mut ctx = current.context.as_object().cloned().unwrap_or_default();
                ctx.insert("work_dir".into(), serde_json::json!(abs_path.to_string_lossy()));
                if !branch.is_empty() && branch != "HEAD" {
                    ctx.insert("branch".into(), serde_json::json!(branch));
                }
                update_issue(
                    &db,
                    &issue_id,
                    UpdateIssueParams {
                        context: Some(Value::Object(ctx)),
                        ..Default::default()
                    },
                )
                .await?;

                if json {
                    println!("{}", serde_json::json!({
                        "issue": issue_id,
                        "work_dir": abs_path.to_string_lossy(),
                        "branch": branch,
                    }));
                } else {
                    println!("Attached {} to issue {issue_id}", abs_path.display());
                }
            }

            WorktreeCommands::Merge { ids } => {
                let mut merged = Vec::new();
                let mut conflicts = Vec::new();

                for id in &ids {
                    let issue = get_issue(&db, id).await?;
                    let branch = issue
                        .context
                        .get("branch")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            anyhow::anyhow!("issue {} has no context.branch", id)
                        })?
                        .to_string();

                    // Best-effort fetch — branch may only exist locally in a worktree
                    let _ = tokio::process::Command::new("git")
                        .args(["fetch", "origin", &branch])
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await;

                    // Try origin/<branch> first, fall back to bare <branch> for local worktrees
                    let origin_branch = format!("origin/{branch}");
                    let merge = tokio::process::Command::new("git")
                        .args(["merge", &origin_branch, "--no-edit"])
                        .status()
                        .await?;

                    let success = if merge.success() {
                        true
                    } else {
                        // Abort and retry with local branch name
                        tokio::process::Command::new("git")
                            .args(["merge", "--abort"])
                            .status()
                            .await?;

                        tokio::process::Command::new("git")
                            .args(["merge", &branch, "--no-edit"])
                            .status()
                            .await?
                            .success()
                    };

                    if success {
                        merged.push(branch);
                    } else {
                        let output = tokio::process::Command::new("git")
                            .args(["diff", "--name-only", "--diff-filter=U"])
                            .output()
                            .await?;
                        let conflicted = String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .map(String::from)
                            .collect::<Vec<_>>();
                        conflicts.extend(conflicted);

                        // Abort the failed merge so subsequent merges can proceed
                        tokio::process::Command::new("git")
                            .args(["merge", "--abort"])
                            .status()
                            .await?;
                    }
                }

                let result = MergeResult { merged, conflicts };
                println!("{}", serde_json::to_string_pretty(&result)?);
            }

            WorktreeCommands::Gc { keep } => {
                let keep_count = match keep {
                    Some(n) => n,
                    None => weaver::settings::get_known(&db, "worktree.keep_count")
                        .await?
                        .parse()
                        .unwrap_or(32),
                };

                let report = weaver::gc_worktrees(&db, keep_count).await?;

                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else if report.removed.is_empty() {
                    println!("No worktrees to remove (active: {}, recent: {})",
                        report.kept_active, report.kept_recent);
                } else {
                    println!("Removed {} worktrees:", report.removed.len());
                    for name in &report.removed {
                        println!("  {name}");
                    }
                    if !report.errors.is_empty() {
                        println!("Errors:");
                        for err in &report.errors {
                            println!("  {err}");
                        }
                    }
                    println!("Kept: {} active, {} recent", report.kept_active, report.kept_recent);
                }
            }
        },

        Commands::Serve {
            addr,
            webhook_url,
        } => {
            // Seed notification settings from CLI flags
            if let Some(url) = webhook_url {
                weaver::settings::set(&db, "notify.generic.url", &url).await.ok();
            }

            let listener = tokio::net::TcpListener::bind(&addr).await?;
            let local_addr = listener.local_addr()?;
            println!("Weaver API server listening on http://{local_addr}");

            let cancel = CancellationToken::new();
            let cancel_clone = cancel.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                cancel_clone.cancel();
            });

            let executor_cancel = cancel.clone();
            let executor_db = db.clone();
            let api_url = format!("http://{local_addr}");
            let runner = default_runner(api_url);
            let hooks: Arc<dyn weaver::ExecutorHooks> =
                Arc::new(NotifyHooks::new(db.clone()));
            let executor = Executor::with_hooks(
                executor_db,
                ExecutorConfig::default(),
                runner,
                hooks,
            );
            tokio::spawn(async move {
                if let Err(e) = executor.run_loop(executor_cancel).await {
                    tracing::error!(error = %e, "Executor run_loop failed");
                }
            });

            weaver::serve(db, listener, cancel).await?;
        }

        Commands::Config { command } => match command {
            ConfigCommands::Set { key, value } => {
                weaver::settings::set(&db, &key, &value).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"key": key, "value": value})
                    );
                } else {
                    println!("{key} = {value}");
                }
            }
            ConfigCommands::Get { key } => {
                match weaver::settings::get(&db, &key).await? {
                    Some(value) => {
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({"key": key, "value": value})
                            );
                        } else {
                            println!("{value}");
                        }
                    }
                    None => {
                        if !json {
                            eprintln!("Key '{key}' not set");
                        }
                        std::process::exit(1);
                    }
                }
            }
            ConfigCommands::List => {
                let all = weaver::settings::get_all(&db).await?;
                if json {
                    let map: serde_json::Map<String, Value> = all
                        .into_iter()
                        .map(|(k, v, _)| (k, Value::String(v)))
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&map)?);
                } else if all.is_empty() {
                    println!("No settings configured.");
                } else {
                    println!("{:<30} {}", "KEY", "VALUE");
                    println!("{}", "-".repeat(60));
                    for (key, value, _) in &all {
                        println!("{key:<30} {value}");
                    }
                }
            }
            ConfigCommands::Delete { key } => {
                let deleted = weaver::settings::delete(&db, &key).await?;
                if deleted {
                    if !json {
                        println!("Deleted '{key}'");
                    }
                } else {
                    if !json {
                        eprintln!("Key '{key}' not set");
                    }
                    std::process::exit(1);
                }
            }
        },

        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "weaver",
                &mut std::io::stdout(),
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(id: &str, title: &str, status: IssueStatus, parent: Option<&str>, tags: &[&str], created_at: &str, completed_at: Option<&str>) -> Issue {
        Issue {
            id: id.to_string(),
            title: title.to_string(),
            body: String::new(),
            status,
            context: serde_json::Value::Null,
            dependencies: vec![],
            num_tries: 1,
            max_tries: 1,
            parent_issue_id: parent.map(|s| s.to_string()),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            priority: 0,
            channel_kind: None,
            origin_ref: None,
            user_id: None,
            error: None,
            created_at: created_at.to_string(),
            updated_at: created_at.to_string(),
            completed_at: completed_at.map(|s| s.to_string()),
            claude_session_id: None,
        }
    }

    #[test]
    fn test_status_icons() {
        assert_eq!(status_icon(&IssueStatus::Completed), "✓");
        assert_eq!(status_icon(&IssueStatus::Failed), "✗");
        assert_eq!(status_icon(&IssueStatus::Running), "◷");
        assert_eq!(status_icon(&IssueStatus::Pending), "○");
        assert_eq!(status_icon(&IssueStatus::Blocked), "⊘");
        assert_eq!(status_icon(&IssueStatus::AwaitingReview), "⏸");
        assert_eq!(status_icon(&IssueStatus::ValidationFailed), "!");
    }

    #[test]
    fn test_format_duration_hours_and_minutes() {
        let d = format_duration("2024-01-01 00:00:00", Some("2024-01-01 01:28:00"));
        assert_eq!(d, "1h 28m");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        let d = format_duration("2024-01-01 00:00:00", Some("2024-01-01 00:05:30"));
        assert_eq!(d, "5m 30s");
    }

    #[test]
    fn test_format_duration_seconds_only() {
        let d = format_duration("2024-01-01 00:00:00", Some("2024-01-01 00:00:45"));
        assert_eq!(d, "45s");
    }

    #[test]
    fn test_format_tree_line_with_tags() {
        let issue = make_issue("abc123", "merge LogStore", IssueStatus::Completed, None, &["iris"], "2024-01-01 00:00:00", Some("2024-01-01 00:28:00"));
        let line = format_tree_line(&issue);
        assert_eq!(line, "abc123 ✓ merge LogStore (28m 0s)");
    }

    #[test]
    fn test_format_tree_line_no_tags() {
        let issue = make_issue("xyz789", "research", IssueStatus::Running, None, &[], "2024-01-01 00:00:00", None);
        let line = format_tree_line(&issue);
        // Running issues have no completed_at, so duration is relative to now; just check prefix
        assert!(line.starts_with("xyz789 ◷ research ("));
    }

    #[test]
    fn test_print_issue_tree_structure() {
        let root = make_issue("root", "root task", IssueStatus::Completed, None, &["iris"], "2024-01-01 00:00:00", Some("2024-01-01 00:28:00"));
        let child1 = make_issue("ch1", "research", IssueStatus::Completed, Some("root"), &[], "2024-01-01 00:01:00", Some("2024-01-01 00:03:00"));
        let child2 = make_issue("ch2", "implement", IssueStatus::Failed, Some("root"), &["impl"], "2024-01-01 00:02:00", Some("2024-01-01 00:13:00"));
        let grandchild = make_issue("gc1", "subtask", IssueStatus::Completed, Some("ch2"), &[], "2024-01-01 00:03:00", Some("2024-01-01 00:05:00"));

        let descendants = vec![child1, child2, grandchild];

        // Verify tree JSON captures the nested structure including grandchildren
        let json = build_tree_json(&root, &descendants);
        assert_eq!(json["children"].as_array().unwrap().len(), 2);
        let ch2_json = &json["children"].as_array().unwrap()[1];
        assert_eq!(ch2_json["children"].as_array().unwrap().len(), 1);
        assert_eq!(ch2_json["children"][0]["id"], "gc1");
    }

    #[test]
    fn test_build_tree_json_structure() {
        let root = make_issue("root", "root task", IssueStatus::Completed, None, &[], "2024-01-01 00:00:00", Some("2024-01-01 00:28:00"));
        let child1 = make_issue("ch1", "child one", IssueStatus::Completed, Some("root"), &[], "2024-01-01 00:01:00", Some("2024-01-01 00:03:00"));
        let child2 = make_issue("ch2", "child two", IssueStatus::Failed, Some("root"), &[], "2024-01-01 00:02:00", Some("2024-01-01 00:10:00"));

        let descendants = vec![child1, child2];
        let json = build_tree_json(&root, &descendants);

        assert_eq!(json["id"], "root");
        assert_eq!(json["title"], "root task");
        let children = json["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["id"], "ch1");
        assert_eq!(children[1]["id"], "ch2");
        assert_eq!(children[1]["status"], "failed");
    }
}
