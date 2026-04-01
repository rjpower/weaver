use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::routing::{delete, get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Db;
use crate::issue::{self, CreateIssueParams, Issue, IssueScope, IssueStatus, ListFilter, UpdateIssueParams, UsageSummary};
use crate::settings;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct IssueResponse {
    pub id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub context: Value,
    pub dependencies: Vec<String>,
    pub num_tries: i32,
    pub max_tries: i32,
    pub parent_issue_id: Option<String>,
    pub tags: Vec<String>,
    pub priority: i32,
    pub channel_kind: Option<String>,
    pub origin_ref: Option<String>,
    pub user: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageSummary>,
}

#[derive(Debug, Serialize)]
pub struct IssueListResponse {
    pub issues: Vec<IssueResponse>,
    pub total: i64,
}

#[derive(Debug, Deserialize)]
pub struct IssueCreateRequest {
    pub title: Option<String>,
    pub body: Option<String>,
    pub context: Option<Value>,
    pub dependencies: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub parent_issue_id: Option<String>,
    pub channel_kind: Option<String>,
    pub origin_ref: Option<String>,
    pub user: Option<String>,
    pub priority: Option<i32>,
    pub max_tries: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct CommentRequest {
    pub author: String,
    pub body: String,
    pub tag: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CommentResponse {
    pub id: i64,
    pub issue_id: String,
    pub author: String,
    pub body: String,
    pub tag: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ListFilters {
    pub status: Option<String>,
    pub tag: Option<String>,
    pub parent_issue_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ReviseRequest {
    pub feedback: String,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ApproveRequest {
    comment: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiffResponse {
    diff: String,
    branch: Option<String>,
    base: String,
    work_dir: Option<String>,
    files_changed: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct PrInfoResponse {
    branch: Option<String>,
    compare_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct FileContentResponse {
    path: String,
    content: String,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

fn issue_to_response(issue: &Issue) -> IssueResponse {
    IssueResponse {
        id: issue.id.clone(),
        title: issue.title.clone(),
        body: issue.body.clone(),
        status: issue.status.as_str().to_string(),
        context: issue.context.clone(),
        dependencies: issue.dependencies.clone(),
        num_tries: issue.num_tries,
        max_tries: issue.max_tries,
        parent_issue_id: issue.parent_issue_id.clone(),
        tags: issue.tags.clone(),
        priority: issue.priority,
        channel_kind: issue.channel_kind.clone(),
        origin_ref: issue.origin_ref.clone(),
        user: issue.user_id.clone(),
        error: issue.error.clone(),
        created_at: issue.created_at.clone(),
        updated_at: issue.updated_at.clone(),
        completed_at: issue.completed_at.clone(),
        claude_session_id: issue.claude_session_id.clone(),
        usage: None,
    }
}

fn issue_to_response_with_usage(issue: &Issue, usage: Option<UsageSummary>) -> IssueResponse {
    let mut resp = issue_to_response(issue);
    resp.usage = usage;
    resp
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list_issues(
    State(db): State<Db>,
    Query(filters): Query<ListFilters>,
) -> Result<Json<IssueListResponse>, (StatusCode, String)> {
    let status_filter = filters
        .status
        .map(|s| s.parse::<IssueStatus>())
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let scope = match filters.parent_issue_id {
        Some(id) => IssueScope::ChildrenOf(id),
        None => IssueScope::TopLevel,
    };

    let filter = ListFilter {
        status: status_filter,
        tag: filters.tag,
        scope,
        limit: Some(filters.limit.unwrap_or(25)),
        offset: Some(filters.offset.unwrap_or(0)),
    };

    let result = issue::list_issues(&db, filter)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let items: Vec<IssueResponse> = result.issues.iter().map(issue_to_response).collect();

    Ok(Json(IssueListResponse {
        issues: items,
        total: result.total,
    }))
}

async fn create_issue(
    State(db): State<Db>,
    Json(req): Json<IssueCreateRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let params = CreateIssueParams {
        title: req.title.unwrap_or_default(),
        body: req.body,
        context: req.context,
        dependencies: req.dependencies.unwrap_or_default(),
        tags: req.tags.unwrap_or_default(),
        priority: req.priority.unwrap_or(0),
        max_tries: req.max_tries,
        parent_issue_id: req.parent_issue_id,
        channel_kind: req.channel_kind,
        origin_ref: req.origin_ref,
        user_id: req.user,
    };

    let issue = match issue::create_issue(&db, params).await {
        Ok(issue) => issue,
        Err(e) => {
            let msg = e.to_string();
            if msg.starts_with("issue already exists") {
                return Err((StatusCode::CONFLICT, msg));
            }
            return Err((StatusCode::INTERNAL_SERVER_ERROR, msg));
        }
    };

    Ok((StatusCode::CREATED, Json(issue_to_response(&issue))))
}

async fn get_issue(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<IssueResponse>, (StatusCode, String)> {
    let issue = issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let usage = issue::get_usage_summary(&db, &id)
        .await
        .ok()
        .flatten();

    Ok(Json(issue_to_response_with_usage(&issue, usage)))
}

#[derive(Deserialize)]
struct IssuePatchRequest {
    tags: Option<Vec<String>>,
    priority: Option<i32>,
    title: Option<String>,
    body: Option<String>,
}

async fn patch_issue(
    State(db): State<Db>,
    Path(id): Path<String>,
    Json(req): Json<IssuePatchRequest>,
) -> Result<Json<IssueResponse>, (StatusCode, String)> {
    issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let updated = issue::update_issue(
        &db,
        &id,
        UpdateIssueParams {
            tags: req.tags,
            priority: req.priority,
            title: req.title,
            body: req.body,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(issue_to_response(&updated)))
}

async fn cancel_issue(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<IssueResponse>, (StatusCode, String)> {
    let issue = issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    match issue.status {
        IssueStatus::Pending | IssueStatus::Running | IssueStatus::AwaitingReview => {}
        _ => {
            return Err((
                StatusCode::CONFLICT,
                format!("Cannot cancel issue in {} state", issue.status),
            ));
        }
    }

    let updated = issue::update_issue(
        &db,
        &id,
        UpdateIssueParams {
            status: Some(IssueStatus::Failed),
            error: Some("Cancelled by user".into()),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(issue_to_response(&updated)))
}

async fn get_comments(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<Vec<CommentResponse>>, (StatusCode, String)> {
    let comments = issue::get_comments(&db, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let responses: Vec<CommentResponse> = comments
        .into_iter()
        .map(|c| CommentResponse {
            id: c.id,
            issue_id: c.issue_id,
            author: c.author,
            body: c.body,
            tag: c.tag,
            created_at: c.created_at,
        })
        .collect();

    Ok(Json(responses))
}

async fn add_comment(
    State(db): State<Db>,
    Path(id): Path<String>,
    Json(req): Json<CommentRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    issue::add_comment(&db, &id, &req.author, &req.body, req.tag.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({"ok": true})))
}

async fn revise_issue(
    State(db): State<Db>,
    Path(id): Path<String>,
    Json(req): Json<ReviseRequest>,
) -> Result<Json<IssueResponse>, (StatusCode, String)> {
    let issue = issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    match issue.status {
        IssueStatus::Completed | IssueStatus::Failed | IssueStatus::ValidationFailed | IssueStatus::AwaitingReview => {}
        _ => {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "Cannot revise issue in {} state (must be completed, failed, validation_failed, or awaiting_review)",
                    issue.status
                ),
            ));
        }
    }

    issue::add_comment(&db, &id, "revision", &req.feedback, Some("revision"))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Clear the session ID so the agent starts fresh with the full prompt and revision feedback,
    // rather than resuming the old completed session (which produces empty results).
    issue::set_claude_session_id(&db, &id, "")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let updated = issue::update_issue(
        &db,
        &id,
        UpdateIssueParams {
            status: Some(IssueStatus::Pending),
            error: Some(String::new()),
            tags: req.tags,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(issue_to_response(&updated)))
}

async fn approve_issue(
    State(db): State<Db>,
    Path(id): Path<String>,
    Json(req): Json<ApproveRequest>,
) -> Result<Json<IssueResponse>, (StatusCode, String)> {
    let issue = issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    if issue.status != IssueStatus::AwaitingReview {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "Cannot approve issue in {} state (must be awaiting_review)",
                issue.status
            ),
        ));
    }

    if let Some(ref comment) = req.comment {
        issue::add_comment(&db, &id, "reviewer", comment, None)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let updated = issue::update_issue(
        &db,
        &id,
        UpdateIssueParams {
            status: Some(IssueStatus::Completed),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(issue_to_response(&updated)))
}

async fn get_issue_diff(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<DiffResponse>, (StatusCode, String)> {
    let issue = issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let branch = issue.context.get("branch").and_then(|v| v.as_str());
    let work_dir = issue.context.get("work_dir").and_then(|v| v.as_str());
    let base = issue
        .context
        .get("base_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main");

    let Some(wd) = work_dir else {
        return Ok(Json(DiffResponse {
            diff: String::new(),
            branch: branch.map(String::from),
            base: base.to_string(),
            work_dir: None,
            files_changed: vec![],
            error: Some("Issue has no work_dir in context".into()),
        }));
    };

    let wd_path = std::path::Path::new(wd);
    if !wd_path.exists() {
        return Ok(Json(DiffResponse {
            diff: String::new(),
            branch: branch.map(String::from),
            base: base.to_string(),
            work_dir: Some(wd.to_string()),
            files_changed: vec![],
            error: Some(format!("Worktree directory does not exist: {wd}")),
        }));
    }

    let diff_output = tokio::process::Command::new("git")
        .args(["diff", base])
        .current_dir(wd_path)
        .output()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to run git diff: {e}"),
            )
        })?;

    let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();

    let names_output = tokio::process::Command::new("git")
        .args(["diff", base, "--name-only"])
        .current_dir(wd_path)
        .output()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to run git diff --name-only: {e}"),
            )
        })?;

    let files_changed: Vec<String> = String::from_utf8_lossy(&names_output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();

    Ok(Json(DiffResponse {
        diff,
        branch: branch.map(String::from),
        base: base.to_string(),
        work_dir: Some(wd.to_string()),
        files_changed,
        error: None,
    }))
}

async fn read_issue_file(
    State(db): State<Db>,
    Path((id, file_path)): Path<(String, String)>,
) -> Result<Json<FileContentResponse>, (StatusCode, String)> {
    let issue = issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let work_dir = issue
        .context
        .get("work_dir")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Issue has no work_dir".into()))?;

    let full_path = std::path::Path::new(work_dir).join(&file_path);

    let canonical_wd = std::fs::canonicalize(work_dir)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Cannot resolve worktree: {e}"),
            )
        })?;
    let canonical_path = std::fs::canonicalize(&full_path)
        .map_err(|_| (StatusCode::NOT_FOUND, "File not found".into()))?;
    if !canonical_path.starts_with(&canonical_wd) {
        return Err((StatusCode::FORBIDDEN, "Path outside worktree".into()));
    }

    let content = tokio::fs::read_to_string(&canonical_path)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Cannot read file: {e}"),
            )
        })?;

    Ok(Json(FileContentResponse {
        path: file_path,
        content,
    }))
}

async fn get_issue_tree_handler(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<Vec<IssueResponse>>, (StatusCode, String)> {
    issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let tree = issue::get_issue_tree(&db, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let responses: Vec<IssueResponse> = tree.iter().map(issue_to_response).collect();
    Ok(Json(responses))
}

async fn get_issue_usage(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<UsageSummary>, (StatusCode, String)> {
    let summary = issue::get_tree_usage_summary(&db, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(summary))
}

async fn get_settings(
    State(db): State<Db>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let all = settings::get_all(&db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let map: serde_json::Map<String, Value> = all
        .into_iter()
        .map(|(k, v, _)| (k, Value::String(v)))
        .collect();
    Ok(Json(Value::Object(map)))
}

async fn update_settings(
    State(db): State<Db>,
    Json(body): Json<serde_json::Map<String, Value>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    for (key, value) in &body {
        let v = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        settings::set(&db, key, &v)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    get_settings(State(db)).await
}

async fn delete_setting(
    State(db): State<Db>,
    Path(key): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let deleted = settings::delete(&db, &key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("Setting '{key}' not found")))
    }
}

async fn get_settings_schema() -> Json<Value> {
    let entries: Vec<Value> = settings::KNOWN_SETTINGS
        .iter()
        .map(|s| {
            serde_json::json!({
                "key": s.key,
                "description": s.description,
                "default": s.default,
            })
        })
        .collect();
    Json(Value::Array(entries))
}

/// Parse a git remote URL (SSH or HTTPS) into `https://github.com/{owner}/{repo}`.
fn parse_github_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    if let Some(rest) = remote.strip_prefix("git@github.com:") {
        let repo = rest.trim_end_matches(".git");
        return Some(format!("https://github.com/{repo}"));
    }
    if remote.starts_with("https://github.com/") {
        let repo = remote.trim_end_matches(".git");
        return Some(repo.to_string());
    }
    None
}

async fn get_pr_info(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<PrInfoResponse>, (StatusCode, String)> {
    let issue = issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let work_dir = issue.context.get("work_dir").and_then(|v| v.as_str());
    let base = issue
        .context
        .get("base_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main");

    let Some(wd) = work_dir else {
        return Ok(Json(PrInfoResponse {
            branch: None,
            compare_url: None,
        }));
    };

    let wd_path = std::path::Path::new(wd);
    if !wd_path.exists() {
        return Ok(Json(PrInfoResponse {
            branch: None,
            compare_url: None,
        }));
    }

    let branch_output = tokio::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(wd_path)
        .output()
        .await
        .ok();
    let branch = branch_output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty());

    let remote_output = tokio::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(wd_path)
        .output()
        .await
        .ok();
    let remote_url = remote_output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|u| !u.is_empty());

    let compare_url = match (&branch, &remote_url) {
        (Some(b), Some(remote)) => {
            parse_github_url(remote).map(|repo_url| format!("{repo_url}/compare/{base}...{b}"))
        }
        _ => None,
    };

    Ok(Json(PrInfoResponse {
        branch,
        compare_url,
    }))
}

// ---------------------------------------------------------------------------
// Streaming events
// ---------------------------------------------------------------------------

/// Truncate tool input/output fields in stream events for display.
/// Full data is stored in the DB; the API returns previews only.
fn truncate_event_for_display(event: crate::runner::StreamEvent) -> crate::runner::StreamEvent {
    use crate::runner::StreamEvent;
    const MAX_PREVIEW: usize = 200;
    match event {
        StreamEvent::ToolUse { tool, input } => {
            let input = if input.len() > MAX_PREVIEW {
                format!("{}...", &input[..MAX_PREVIEW])
            } else {
                input
            };
            StreamEvent::ToolUse { tool, input }
        }
        StreamEvent::ToolResult { tool, output } => {
            let output = if output.len() > MAX_PREVIEW {
                format!("{}...", &output[..MAX_PREVIEW])
            } else {
                output
            };
            StreamEvent::ToolResult { tool, output }
        }
        other => other,
    }
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    after_seq: Option<i64>,
}

async fn stream_issue_events(
    State(db): State<Db>,
    Path(id): Path<String>,
    Query(q): Query<StreamQuery>,
) -> Result<
    Sse<impl futures_core::Stream<Item = Result<SseEvent, Infallible>>>,
    (StatusCode, String),
> {
    // Verify issue exists
    issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let stream = async_stream::stream! {
        let mut last_seq = q.after_seq.unwrap_or(0);
        loop {
            let events = issue::get_events_since(&db, &id, last_seq)
                .await
                .unwrap_or_default();
            let no_events = events.is_empty();
            for (seq, event) in events {
                last_seq = seq;
                let display_event = truncate_event_for_display(event);
                if let Ok(data) = serde_json::to_string(&display_event) {
                    yield Ok(SseEvent::default().id(seq.to_string()).data(data));
                }
            }
            // Stop streaming once the issue is terminal and no new events
            if let Ok(iss) = issue::get_issue(&db, &id).await {
                if iss.status.is_terminal() && no_events {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Debug, Serialize)]
struct EventsResponse {
    events: Vec<EventEntry>,
}

#[derive(Debug, Serialize)]
struct EventEntry {
    seq: i64,
    #[serde(flatten)]
    event: crate::runner::StreamEvent,
}

async fn get_issue_events(
    State(db): State<Db>,
    Path(id): Path<String>,
    Query(q): Query<StreamQuery>,
) -> Result<Json<EventsResponse>, (StatusCode, String)> {
    issue::get_issue(&db, &id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Issue {id} not found")))?;

    let after_seq = q.after_seq.unwrap_or(0);
    let events = issue::get_events_since(&db, &id, after_seq)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(EventsResponse {
        events: events
            .into_iter()
            .map(|(seq, event)| EventEntry {
                seq,
                event: truncate_event_for_display(event),
            })
            .collect(),
    }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(db: Db) -> Router {
    Router::new()
        .route("/api/issues", get(list_issues).post(create_issue))
        .route("/api/issues/{id}", get(get_issue).patch(patch_issue))
        .route("/api/issues/{id}/cancel", post(cancel_issue))
        .route("/api/issues/{id}/revise", post(revise_issue))
        .route(
            "/api/issues/{id}/comments",
            get(get_comments).post(add_comment),
        )
        .route("/api/issues/{id}/approve", post(approve_issue))
        .route("/api/issues/{id}/tree", get(get_issue_tree_handler))
        .route("/api/issues/{id}/diff", get(get_issue_diff))
        .route("/api/issues/{id}/usage", get(get_issue_usage))
        .route("/api/issues/{id}/pr", get(get_pr_info))
        .route("/api/issues/{id}/files/{*path}", get(read_issue_file))
        .route("/api/issues/{id}/stream", get(stream_issue_events))
        .route("/api/issues/{id}/events", get(get_issue_events))
        .route("/api/settings", get(get_settings).put(update_settings))
        .route("/api/settings/schema", get(get_settings_schema))
        .route("/api/settings/{key}", delete(delete_setting))
        .with_state(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn test_db() -> Db {
        crate::db::connect_in_memory().await.unwrap()
    }

    fn app(db: Db) -> Router {
        router(db)
    }

    async fn read_json(resp: axum::http::Response<Body>) -> Value {
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn list_issues_empty() {
        let db = test_db().await;
        let req = Request::builder()
            .uri("/api/issues")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        assert_eq!(json["issues"], serde_json::json!([]));
        assert_eq!(json["total"], 0);
    }

    #[tokio::test]
    async fn create_and_get_issue() {
        let db = test_db().await;

        let body = serde_json::json!({"title": "test", "body": "do it"});
        let req = Request::builder()
            .uri("/api/issues")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let created = read_json(resp).await;
        let id = created["id"].as_str().unwrap();
        assert_eq!(created["title"], "test");
        assert_eq!(created["body"], "do it");
        assert_eq!(created["status"], "pending");

        let req = Request::builder()
            .uri(format!("/api/issues/{id}"))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let fetched = read_json(resp).await;
        assert_eq!(fetched["id"], id);
        assert_eq!(fetched["title"], "test");
    }

    #[tokio::test]
    async fn cancel_pending_issue() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "to cancel".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let req = Request::builder()
            .uri(format!("/api/issues/{}/cancel", issue.id))
            .method("POST")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        assert_eq!(json["status"], "failed");
    }

    #[tokio::test]
    async fn cancel_completed_returns_conflict() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "already done".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let req = Request::builder()
            .uri(format!("/api/issues/{}/cancel", issue.id))
            .method("POST")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn comments_roundtrip() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "with comments".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let body = serde_json::json!({"author": "test", "body": "hello"});
        let req = Request::builder()
            .uri(format!("/api/issues/{}/comments", issue.id))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let req = Request::builder()
            .uri(format!("/api/issues/{}/comments", issue.id))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        let comments = json.as_array().unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0]["author"], "test");
        assert_eq!(comments[0]["body"], "hello");
    }

    #[tokio::test]
    async fn revise_completed_issue() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "to revise".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::add_comment(&db, &issue.id, "agent", "previous work output", Some("result")).await.unwrap();

        let body = serde_json::json!({"feedback": "Please fix the formatting"});
        let req = Request::builder()
            .uri(format!("/api/issues/{}/revise", issue.id))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        assert_eq!(json["status"], "pending");

        // Comments: result + revision feedback
        let comments = issue::get_comments(&db, &issue.id).await.unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].author, "agent");
        assert_eq!(comments[0].tag.as_deref(), Some("result"));
        assert_eq!(comments[1].author, "revision");
        assert_eq!(comments[1].body, "Please fix the formatting");

        // Session ID must be cleared so the agent starts fresh
        let refreshed = issue::get_issue(&db, &issue.id).await.unwrap();
        assert!(refreshed.claude_session_id.is_none());
    }

    #[tokio::test]
    async fn revise_pending_issue_returns_conflict() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "still pending".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let body = serde_json::json!({"feedback": "do better"});
        let req = Request::builder()
            .uri(format!("/api/issues/{}/revise", issue.id))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn patch_issue_updates_tags() {
        let db = test_db().await;

        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "taggable".into(),
                tags: vec!["old".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let body = serde_json::json!({"tags": ["new", "shiny"]});
        let req = Request::builder()
            .uri(format!("/api/issues/{}", issue.id))
            .method("PATCH")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let result = read_json(resp).await;
        assert_eq!(result["tags"], serde_json::json!(["new", "shiny"]));
    }

    #[tokio::test]
    async fn create_issue_returns_201() {
        let db = test_db().await;

        let body = serde_json::json!({"title": "new issue"});
        let req = Request::builder()
            .uri("/api/issues")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn list_issues_filters_by_status() {
        let db = test_db().await;

        issue::create_issue(
            &db,
            CreateIssueParams {
                title: "pending one".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let completed = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "completed one".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        issue::update_issue(
            &db,
            &completed.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let req = Request::builder()
            .uri("/api/issues?status=pending")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        let issues = json["issues"].as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["title"], "pending one");
    }

    #[tokio::test]
    async fn approve_awaiting_review_issue() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "to review".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                status: Some(IssueStatus::AwaitingReview),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let body = serde_json::json!({"comment": "looks good"});
        let req = Request::builder()
            .uri(format!("/api/issues/{}/approve", issue.id))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        assert_eq!(json["status"], "completed");

        let comments = issue::get_comments(&db, &issue.id).await.unwrap();
        assert!(comments
            .iter()
            .any(|c| c.author == "reviewer" && c.body == "looks good"));
    }

    #[tokio::test]
    async fn approve_non_review_issue_returns_conflict() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "still pending".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let body = serde_json::json!({});
        let req = Request::builder()
            .uri(format!("/api/issues/{}/approve", issue.id))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn revise_awaiting_review_issue() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "to review".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                status: Some(IssueStatus::AwaitingReview),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::add_comment(&db, &issue.id, "agent", "my design doc", Some("result")).await.unwrap();

        let body = serde_json::json!({"feedback": "need more detail on error handling"});
        let req = Request::builder()
            .uri(format!("/api/issues/{}/revise", issue.id))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        assert_eq!(json["status"], "pending");
    }

    #[tokio::test]
    async fn cancel_awaiting_review_issue() {
        let db = test_db().await;
        let issue = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "reviewing".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        issue::update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                status: Some(IssueStatus::AwaitingReview),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let req = Request::builder()
            .uri(format!("/api/issues/{}/cancel", issue.id))
            .method("POST")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = read_json(resp).await;
        assert_eq!(json["status"], "failed");
    }

    #[tokio::test]
    async fn get_issue_tree_returns_descendants() {
        let db = test_db().await;
        let parent = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "root".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let child = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "child".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let _grandchild = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "grandchild".into(),
                parent_issue_id: Some(child.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let req = Request::builder()
            .uri(format!("/api/issues/{}/tree", parent.id))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = read_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn parse_github_url_ssh() {
        assert_eq!(
            parse_github_url("git@github.com:user/repo.git"),
            Some("https://github.com/user/repo".into())
        );
    }

    #[test]
    fn parse_github_url_https() {
        assert_eq!(
            parse_github_url("https://github.com/user/repo.git"),
            Some("https://github.com/user/repo".into())
        );
    }

    #[test]
    fn parse_github_url_https_no_git_suffix() {
        assert_eq!(
            parse_github_url("https://github.com/user/repo"),
            Some("https://github.com/user/repo".into())
        );
    }

    #[test]
    fn parse_github_url_non_github() {
        assert_eq!(parse_github_url("https://gitlab.com/user/repo.git"), None);
    }

    #[tokio::test]
    async fn create_issue_returns_conflict_on_duplicate() {
        let db = test_db().await;

        let body = serde_json::json!({"title": "dup test"});
        let req = Request::builder()
            .uri("/api/issues")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let req = Request::builder()
            .uri("/api/issues")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn get_events_empty() {
        let db = test_db().await;
        let iss = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let req = Request::builder()
            .uri(format!("/api/issues/{}/events", iss.id))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = read_json(resp).await;
        assert_eq!(json["events"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn get_events_returns_stored() {
        let db = test_db().await;
        let iss = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let event = crate::runner::StreamEvent::Text {
            text: "hello".into(),
        };
        issue::insert_event(&db, &iss.id, 1, &event).await.unwrap();

        let req = Request::builder()
            .uri(format!("/api/issues/{}/events", iss.id))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = read_json(resp).await;
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["kind"], "text");
        assert_eq!(events[0]["text"], "hello");
        assert_eq!(events[0]["seq"], 1);
    }

    #[tokio::test]
    async fn get_events_after_seq_filters() {
        let db = test_db().await;
        let iss = issue::create_issue(
            &db,
            CreateIssueParams {
                title: "test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        for i in 1..=3 {
            let event = crate::runner::StreamEvent::Text {
                text: format!("msg{i}"),
            };
            issue::insert_event(&db, &iss.id, i, &event).await.unwrap();
        }

        let req = Request::builder()
            .uri(format!("/api/issues/{}/events?after_seq=2", iss.id))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        let json = read_json(resp).await;
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["text"], "msg3");
        assert_eq!(events[0]["seq"], 3);
    }

    #[tokio::test]
    async fn get_events_404_for_missing_issue() {
        let db = test_db().await;
        let req = Request::builder()
            .uri("/api/issues/nonexistent/events")
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let resp = app(db).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
