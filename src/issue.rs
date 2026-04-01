use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Db;

const ISSUE_COLUMNS: &str = "id, title, body, status, prompt, context, dependencies, \
    num_tries, max_tries, parent_issue_id, tags, priority, channel_kind, origin_ref, \
    user_id, error, created_at, updated_at, completed_at, claude_session_id";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub body: String,
    pub status: IssueStatus,
    pub context: Value,
    pub dependencies: Vec<String>,
    pub num_tries: i32,
    pub max_tries: i32,
    pub parent_issue_id: Option<String>,
    pub tags: Vec<String>,
    pub priority: i32,
    pub channel_kind: Option<String>,
    pub origin_ref: Option<String>,
    pub user_id: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub claude_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    Pending,
    Running,
    Completed,
    Failed,
    ValidationFailed,
    Blocked,
    AwaitingReview,
}

impl IssueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::ValidationFailed => "validation_failed",
            Self::Blocked => "blocked",
            Self::AwaitingReview => "awaiting_review",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::ValidationFailed | Self::Blocked
        )
    }
}

impl std::fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for IssueStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "validation_failed" => Ok(Self::ValidationFailed),
            "blocked" => Ok(Self::Blocked),
            "awaiting_review" => Ok(Self::AwaitingReview),
            other => anyhow::bail!("unknown issue status: {other}"),
        }
    }
}

#[derive(Debug, Default)]
pub struct CreateIssueParams {
    pub title: String,
    pub body: Option<String>,
    pub context: Option<Value>,
    pub dependencies: Vec<String>,
    pub tags: Vec<String>,
    pub priority: i32,
    pub max_tries: Option<i32>,
    pub parent_issue_id: Option<String>,
    pub channel_kind: Option<String>,
    pub origin_ref: Option<String>,
    pub user_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct UpdateIssueParams {
    pub title: Option<String>,
    pub body: Option<String>,
    pub context: Option<Value>,
    pub dependencies: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub priority: Option<i32>,
    pub status: Option<IssueStatus>,
    pub error: Option<String>,
    pub num_tries: Option<i32>,
}

#[derive(Debug, Default)]
pub enum IssueScope {
    #[default]
    All,
    TopLevel,
    ChildrenOf(String),
}

#[derive(Debug, Default)]
pub struct ListFilter {
    pub status: Option<IssueStatus>,
    pub tag: Option<String>,
    pub scope: IssueScope,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub struct ListResult {
    pub issues: Vec<Issue>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: i64,
    pub issue_id: String,
    pub author: String,
    pub body: String,
    pub tag: Option<String>,
    pub created_at: String,
}

/// Raw row from SQLite — JSON fields are strings, status is a string.
#[derive(sqlx::FromRow)]
struct IssueRow {
    id: String,
    title: String,
    body: String,
    status: String,
    #[allow(dead_code)]
    prompt: Option<String>,
    context: String,
    dependencies: String,
    num_tries: i32,
    max_tries: i32,
    parent_issue_id: Option<String>,
    tags: String,
    priority: i32,
    channel_kind: Option<String>,
    origin_ref: Option<String>,
    user_id: Option<String>,
    error: Option<String>,
    created_at: String,
    updated_at: String,
    completed_at: Option<String>,
    claude_session_id: Option<String>,
}

fn row_to_issue(row: IssueRow) -> anyhow::Result<Issue> {
    Ok(Issue {
        id: row.id,
        title: row.title,
        body: row.body,
        status: row.status.parse()?,
        context: serde_json::from_str(&row.context)?,
        dependencies: serde_json::from_str(&row.dependencies)?,
        num_tries: row.num_tries,
        max_tries: row.max_tries,
        parent_issue_id: row.parent_issue_id,
        tags: serde_json::from_str(&row.tags)?,
        priority: row.priority,
        channel_kind: row.channel_kind,
        origin_ref: row.origin_ref,
        user_id: row.user_id,
        error: row.error,
        created_at: row.created_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
        claude_session_id: row.claude_session_id,
    })
}

pub async fn find_active_issue_by_title(db: &Db, title: &str) -> anyhow::Result<Option<Issue>> {
    let sql = format!(
        "SELECT {ISSUE_COLUMNS} FROM issues WHERE title = ? \
         AND status NOT IN ('completed', 'failed', 'validation_failed', 'blocked') LIMIT 1"
    );
    let row: Option<IssueRow> = sqlx::query_as(&sql)
    .bind(title)
    .fetch_optional(db)
    .await?;
    row.map(row_to_issue).transpose()
}

fn generate_id() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    (0..8).map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char).collect()
}

/// Verify all dependency IDs are non-empty and exist in the DB.
async fn validate_dependencies(db: &Db, deps: &[String]) -> anyhow::Result<Vec<String>> {
    if deps.iter().any(|d| d.is_empty()) {
        anyhow::bail!("dependency ID must not be empty");
    }
    let deps: Vec<String> = deps.to_vec();
    for dep_id in &deps {
        let exists: bool =
            sqlx::query_scalar("SELECT COUNT(*) > 0 FROM issues WHERE id = ?")
                .bind(dep_id)
                .fetch_one(db)
                .await?;
        if !exists {
            anyhow::bail!("dependency {dep_id} does not exist");
        }
    }
    Ok(deps)
}

pub async fn create_issue(db: &Db, params: CreateIssueParams) -> anyhow::Result<Issue> {
    if let Some(existing) = find_active_issue_by_title(db, &params.title).await? {
        anyhow::bail!("issue already exists with title '{}': {}", params.title, existing.id);
    }

    let dependencies = validate_dependencies(db, &params.dependencies).await?;

    // Reject dependencies that would create a deadlock: a child depending on
    // its own parent (or any ancestor) can never start because the ancestor is
    // running and waiting for this child to complete.
    if !dependencies.is_empty() {
        if let Some(ref parent_id) = params.parent_issue_id {
            let mut ancestors = std::collections::HashSet::new();
            let mut cur = Some(parent_id.clone());
            while let Some(aid) = cur {
                ancestors.insert(aid.clone());
                let parent: Option<(Option<String>,)> =
                    sqlx::query_as("SELECT parent_issue_id FROM issues WHERE id = ?")
                        .bind(&aid)
                        .fetch_optional(db)
                        .await?;
                cur = parent.and_then(|(p,)| p);
            }
            for dep in &dependencies {
                if ancestors.contains(dep) {
                    anyhow::bail!(
                        "dependency {} is an ancestor of this issue — this would deadlock \
                         (ancestor is waiting for children to complete)",
                        dep
                    );
                }
            }
        }
    }

    let id = generate_id();
    let body = params.body.unwrap_or_default();

    // Inherit work_dir from parent if not explicitly set
    let mut context_obj = match params.context {
        Some(Value::Object(m)) => m,
        Some(other) => serde_json::from_value(other).unwrap_or_default(),
        None => Default::default(),
    };
    // Only inherit parent's work_dir when same_worktree is explicitly set.
    // Otherwise, leave work_dir empty so the executor creates a new worktree
    // branching off the parent's branch.
    let same_worktree = context_obj
        .get("same_worktree")
        .and_then(|v| v.as_bool())
        == Some(true);
    if same_worktree && !context_obj.contains_key("work_dir") {
        if let Some(ref parent_id) = params.parent_issue_id {
            if let Ok(parent) = get_issue(db, parent_id).await {
                if let Some(wd) = parent.context.get("work_dir") {
                    context_obj.insert("work_dir".into(), wd.clone());
                }
            }
        }
    }
    let context = serde_json::to_string(&Value::Object(context_obj))?;
    let deps = serde_json::to_string(&dependencies)?;
    let tags = serde_json::to_string(&params.tags)?;
    let max_tries = params.max_tries.unwrap_or(3);

    sqlx::query(
        "INSERT INTO issues (id, title, body, status, context, \
         dependencies, max_tries, parent_issue_id, tags, priority, channel_kind, origin_ref, user_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&params.title)
    .bind(&body)
    .bind(IssueStatus::Pending.as_str())
    .bind(&context)
    .bind(&deps)
    .bind(max_tries)
    .bind(&params.parent_issue_id)
    .bind(&tags)
    .bind(params.priority)
    .bind(&params.channel_kind)
    .bind(&params.origin_ref)
    .bind(&params.user_id)
    .execute(db)
    .await?;

    get_issue(db, &id).await
}

pub async fn get_issue(db: &Db, id: &str) -> anyhow::Result<Issue> {
    let sql = format!("SELECT {ISSUE_COLUMNS} FROM issues WHERE id = ?");
    let row: IssueRow = sqlx::query_as(&sql)
        .bind(id)
        .fetch_one(db)
        .await?;
    row_to_issue(row)
}

pub async fn update_issue(db: &Db, id: &str, params: UpdateIssueParams) -> anyhow::Result<Issue> {
    let mut sets = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if let Some(ref title) = params.title {
        sets.push("title = ?");
        binds.push(title.clone());
    }
    if let Some(ref body) = params.body {
        sets.push("body = ?");
        binds.push(body.clone());
    }
    if let Some(ref ctx) = params.context {
        sets.push("context = ?");
        binds.push(serde_json::to_string(ctx)?);
    }
    if let Some(ref deps) = params.dependencies {
        let deps = validate_dependencies(db, deps).await?;
        sets.push("dependencies = ?");
        binds.push(serde_json::to_string(&deps)?);
    }
    if let Some(ref tags) = params.tags {
        sets.push("tags = ?");
        binds.push(serde_json::to_string(tags)?);
    }
    if let Some(priority) = params.priority {
        sets.push("priority = ?");
        binds.push(priority.to_string());
    }
    if let Some(ref status) = params.status {
        sets.push("status = ?");
        binds.push(status.as_str().to_string());
        if *status == IssueStatus::Completed
            || *status == IssueStatus::Failed
            || *status == IssueStatus::ValidationFailed
        {
            sets.push("completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
        }
    }
    if let Some(ref error) = params.error {
        sets.push("error = ?");
        binds.push(error.clone());
    }
    if let Some(num_tries) = params.num_tries {
        sets.push("num_tries = ?");
        binds.push(num_tries.to_string());
    }

    if sets.is_empty() {
        return get_issue(db, id).await;
    }

    sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");

    let sql = format!("UPDATE issues SET {} WHERE id = ?", sets.join(", "));
    let mut query = sqlx::query(&sql);
    for bind in &binds {
        query = query.bind(bind);
    }
    query = query.bind(id);
    query.execute(db).await?;

    get_issue(db, id).await
}

pub async fn set_claude_session_id(db: &Db, issue_id: &str, session_id: &str) -> anyhow::Result<()> {
    let sid = if session_id.is_empty() { None } else { Some(session_id) };
    sqlx::query("UPDATE issues SET claude_session_id = ? WHERE id = ?")
        .bind(sid)
        .bind(issue_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn list_issues(db: &Db, filter: ListFilter) -> anyhow::Result<ListResult> {
    let mut conditions = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    let is_top_level = matches!(filter.scope, IssueScope::TopLevel);

    match &filter.scope {
        IssueScope::All => {}
        IssueScope::TopLevel => {
            conditions.push("parent_issue_id IS NULL".to_string());
        }
        IssueScope::ChildrenOf(parent_id) => {
            conditions.push("parent_issue_id = ?".to_string());
            binds.push(parent_id.clone());
        }
    }

    if let Some(ref status) = filter.status {
        conditions.push("status = ?".to_string());
        binds.push(status.as_str().to_string());
    }
    if let Some(ref tag) = filter.tag {
        conditions.push(
            "EXISTS (SELECT 1 FROM json_each(tags) WHERE json_each.value = ?)".to_string(),
        );
        binds.push(tag.clone());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM issues{where_clause}");
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for bind in &binds {
        count_query = count_query.bind(bind);
    }
    let total: i64 = count_query.fetch_one(db).await?;

    let limit = filter.limit.unwrap_or(100);
    let offset = filter.offset.unwrap_or(0);
    let sql = format!(
        "SELECT {ISSUE_COLUMNS} FROM issues{where_clause} \
         ORDER BY \
           CASE status \
             WHEN 'running' THEN 0 \
             WHEN 'pending' THEN 1 \
             WHEN 'awaiting_review' THEN 2 \
             WHEN 'blocked' THEN 3 \
             WHEN 'failed' THEN 4 \
             WHEN 'validation_failed' THEN 5 \
             WHEN 'completed' THEN 6 \
             ELSE 7 \
           END, \
           priority DESC, \
           created_at DESC \
         LIMIT ? OFFSET ?"
    );

    let mut query = sqlx::query_as::<_, IssueRow>(&sql);
    for bind in &binds {
        query = query.bind(bind);
    }
    query = query.bind(limit);
    query = query.bind(offset);

    let rows: Vec<IssueRow> = query.fetch_all(db).await?;
    let mut issues = Vec::with_capacity(rows.len());
    for row in rows {
        issues.push(row_to_issue(row)?);
    }

    if !is_top_level {
        return Ok(ListResult { issues, total });
    }

    // TopLevel: recursively fetch all descendants and interleave in DFS tree order
    let parent_ids: Vec<String> = issues.iter().map(|i| i.id.clone()).collect();
    if parent_ids.is_empty() {
        return Ok(ListResult { issues, total });
    }

    let mut children_by_parent: HashMap<String, Vec<Issue>> = HashMap::new();

    let mut queue = parent_ids;
    while !queue.is_empty() {
        let placeholders = queue.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let children_sql = format!(
            "SELECT {ISSUE_COLUMNS} FROM issues WHERE parent_issue_id IN ({placeholders}) \
             ORDER BY created_at ASC"
        );
        let mut children_query = sqlx::query_as::<_, IssueRow>(&children_sql);
        for id in &queue {
            children_query = children_query.bind(id);
        }
        let child_rows: Vec<IssueRow> = children_query.fetch_all(db).await?;

        queue.clear();
        for row in child_rows {
            let parent_id = row.parent_issue_id.clone().unwrap_or_default();
            let child = row_to_issue(row)?;
            queue.push(child.id.clone());
            children_by_parent.entry(parent_id).or_default().push(child);
        }
    }

    fn interleave(
        id: &str,
        children_by_parent: &mut HashMap<String, Vec<Issue>>,
        out: &mut Vec<Issue>,
    ) {
        if let Some(children) = children_by_parent.remove(id) {
            for child in children {
                let child_id = child.id.clone();
                out.push(child);
                interleave(&child_id, children_by_parent, out);
            }
        }
    }

    let parents = std::mem::take(&mut issues);
    for parent in parents {
        let pid = parent.id.clone();
        issues.push(parent);
        interleave(&pid, &mut children_by_parent, &mut issues);
    }

    Ok(ListResult { issues, total })
}

/// Recursively fetch all descendants of an issue (children, grandchildren, etc).
/// Returns a flat Vec ordered by created_at.
pub async fn get_issue_tree(db: &Db, root_id: &str) -> anyhow::Result<Vec<Issue>> {
    let mut result = Vec::new();
    let mut queue = vec![root_id.to_string()];

    while let Some(parent_id) = queue.pop() {
        let children = list_issues(
            db,
            ListFilter {
                scope: IssueScope::ChildrenOf(parent_id),
                limit: Some(1000),
                ..Default::default()
            },
        )
        .await?
        .issues;

        for child in children {
            queue.push(child.id.clone());
            result.push(child);
        }
    }

    result.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(result)
}

pub async fn add_comment(
    db: &Db,
    issue_id: &str,
    author: &str,
    body: &str,
    tag: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO issue_comments (issue_id, author, body, tag) VALUES (?, ?, ?, ?)",
    )
    .bind(issue_id)
    .bind(author)
    .bind(body)
    .bind(tag)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_comments(db: &Db, issue_id: &str) -> anyhow::Result<Vec<Comment>> {
    let rows: Vec<(i64, String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, issue_id, author, body, tag, created_at FROM issue_comments \
         WHERE issue_id = ? ORDER BY created_at ASC",
    )
    .bind(issue_id)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, issue_id, author, body, tag, created_at)| Comment {
            id,
            issue_id,
            author,
            body,
            tag,
            created_at,
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
}

/// Rolled-up usage across all execution attempts for an issue.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageSummary {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub model: Option<String>,
    pub attempts: i64,
}

pub async fn insert_usage(db: &Db, issue_id: &str, stats: &UsageStats) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO issue_usage (issue_id, input_tokens, output_tokens, model, cost_usd) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(issue_id)
    .bind(stats.input_tokens)
    .bind(stats.output_tokens)
    .bind(&stats.model)
    .bind(stats.cost_usd)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_usage_summary(db: &Db, issue_id: &str) -> anyhow::Result<Option<UsageSummary>> {
    let row: Option<(i64, i64, f64, i64)> = sqlx::query_as(
        "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), \
         COALESCE(SUM(cost_usd), 0.0), COUNT(*) \
         FROM issue_usage WHERE issue_id = ?",
    )
    .bind(issue_id)
    .fetch_optional(db)
    .await?;

    match row {
        Some((_, _, _, 0)) => Ok(None),
        Some((input, output, cost, attempts)) => {
            let model: Option<String> = sqlx::query_scalar(
                "SELECT model FROM issue_usage WHERE issue_id = ? ORDER BY id DESC LIMIT 1",
            )
            .bind(issue_id)
            .fetch_optional(db)
            .await?
            .flatten();

            Ok(Some(UsageSummary {
                input_tokens: input,
                output_tokens: output,
                cost_usd: cost,
                model,
                attempts,
            }))
        }
        None => Ok(None),
    }
}

/// Get aggregated usage across an issue and all its descendants.
pub async fn get_tree_usage_summary(db: &Db, root_id: &str) -> anyhow::Result<UsageSummary> {
    let tree = get_issue_tree(db, root_id).await?;
    let mut ids = vec![root_id.to_string()];
    ids.extend(tree.into_iter().map(|i| i.id));

    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), \
         COALESCE(SUM(cost_usd), 0.0), COUNT(*) \
         FROM issue_usage WHERE issue_id IN ({placeholders})"
    );
    let mut query = sqlx::query_as::<_, (i64, i64, f64, i64)>(&sql);
    for id in &ids {
        query = query.bind(id);
    }
    let (input, output, cost, attempts) = query.fetch_one(db).await?;

    Ok(UsageSummary {
        input_tokens: input,
        output_tokens: output,
        cost_usd: cost,
        model: None,
        attempts,
    })
}

pub async fn clear_events(db: &Db, issue_id: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM issue_events WHERE issue_id = ?")
        .bind(issue_id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Stream events
// ---------------------------------------------------------------------------

pub async fn insert_event(
    db: &Db,
    issue_id: &str,
    seq: i64,
    event: &crate::runner::StreamEvent,
) -> anyhow::Result<()> {
    let kind = match event {
        crate::runner::StreamEvent::Init { .. } => "init",
        crate::runner::StreamEvent::Text { .. } => "text",
        crate::runner::StreamEvent::ToolUse { .. } => "tool_use",
        crate::runner::StreamEvent::ToolResult { .. } => "tool_result",
        crate::runner::StreamEvent::Result { .. } => "result",
        crate::runner::StreamEvent::Error { .. } => "error",
    };
    let data = serde_json::to_string(event)?;
    sqlx::query(
        "INSERT INTO issue_events (issue_id, seq, kind, data) VALUES (?, ?, ?, ?)",
    )
    .bind(issue_id)
    .bind(seq)
    .bind(kind)
    .bind(data)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_events_since(
    db: &Db,
    issue_id: &str,
    after_seq: i64,
) -> anyhow::Result<Vec<(i64, crate::runner::StreamEvent)>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT seq, data FROM issue_events WHERE issue_id = ? AND seq > ? ORDER BY seq",
    )
    .bind(issue_id)
    .bind(after_seq)
    .fetch_all(db)
    .await?;

    rows.into_iter()
        .map(|(seq, data)| {
            let event: crate::runner::StreamEvent = serde_json::from_str(&data)?;
            Ok((seq, event))
        })
        .collect()
}

/// Get the most recent result comment for an issue.
pub async fn get_result_comment(db: &Db, issue_id: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT body FROM issue_comments WHERE issue_id = ? AND tag = 'result' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(issue_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(body,)| body))
}

/// Batch-fetch the most recent result comment for each issue in a set of IDs.
pub async fn get_result_comments_batch(
    db: &Db,
    issue_ids: &[String],
) -> anyhow::Result<HashMap<String, String>> {
    if issue_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = issue_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT issue_id, body FROM issue_comments \
         WHERE issue_id IN ({placeholders}) AND tag = 'result' \
         ORDER BY created_at DESC"
    );
    let mut query = sqlx::query_as::<_, (String, String)>(&sql);
    for id in issue_ids {
        query = query.bind(id);
    }
    let rows: Vec<(String, String)> = query.fetch_all(db).await?;
    let mut map = HashMap::new();
    for (issue_id, body) in rows {
        map.entry(issue_id).or_insert(body);
    }
    Ok(map)
}

pub fn estimate_cost(input_tokens: i64, output_tokens: i64, model: Option<&str>) -> f64 {
    let (input_rate, output_rate) = match model {
        Some(m) if m.contains("opus") => (15.0, 75.0),
        Some(m) if m.contains("haiku") => (0.25, 1.25),
        _ => (3.0, 15.0), // sonnet default
    };
    (input_tokens as f64 * input_rate + output_tokens as f64 * output_rate) / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_db() -> Db {
        db::connect_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn create_and_get_issue() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Test issue".into(),
                body: Some("body text".into()),
                tags: vec!["tracking".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(issue.title, "Test issue");
        assert_eq!(issue.body, "body text");
        assert_eq!(issue.status, IssueStatus::Pending);
        assert_eq!(issue.tags, vec!["tracking"]);

        let fetched = get_issue(&db, &issue.id).await.unwrap();
        assert_eq!(fetched.id, issue.id);
    }

    #[tokio::test]
    async fn update_issue_fields() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Original".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let updated = update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                title: Some("Updated".into()),
                status: Some(IssueStatus::Pending),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.title, "Updated");
        assert_eq!(updated.status, IssueStatus::Pending);
    }

    #[tokio::test]
    async fn list_issues_filters_by_status() {
        let db = test_db().await;
        create_issue(
            &db,
            CreateIssueParams {
                title: "Pending issue".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let completed = create_issue(
            &db,
            CreateIssueParams {
                title: "Completed issue".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        update_issue(
            &db,
            &completed.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let pending = list_issues(
            &db,
            ListFilter {
                status: Some(IssueStatus::Pending),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(pending.issues.len(), 1);
        assert_eq!(pending.total, 1);
        assert_eq!(pending.issues[0].title, "Pending issue");

        let all = list_issues(&db, ListFilter::default()).await.unwrap();
        assert_eq!(all.issues.len(), 2);
        assert_eq!(all.total, 2);
    }

    #[tokio::test]
    async fn list_issues_filters_by_tag() {
        let db = test_db().await;
        create_issue(
            &db,
            CreateIssueParams {
                title: "Tagged".into(),
                tags: vec!["auth".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        create_issue(
            &db,
            CreateIssueParams {
                title: "Untagged".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let filtered = list_issues(
            &db,
            ListFilter {
                tag: Some("auth".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(filtered.issues.len(), 1);
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.issues[0].title, "Tagged");
    }

    #[tokio::test]
    async fn add_and_get_comments() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Commented issue".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        add_comment(&db, &issue.id, "weaver", "First comment", None).await.unwrap();
        add_comment(&db, &issue.id, "user", "Second comment", None).await.unwrap();

        let comments = get_comments(&db, &issue.id).await.unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].author, "weaver");
        assert_eq!(comments[0].body, "First comment");
        assert_eq!(comments[1].author, "user");
    }

    #[tokio::test]
    async fn get_issue_tree_two_levels() {
        let db = test_db().await;
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "Root".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child1 = create_issue(
            &db,
            CreateIssueParams {
                title: "Child 1".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child2 = create_issue(
            &db,
            CreateIssueParams {
                title: "Child 2".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let grandchild = create_issue(
            &db,
            CreateIssueParams {
                title: "Grandchild".into(),
                parent_issue_id: Some(child1.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let tree = get_issue_tree(&db, &parent.id).await.unwrap();
        assert_eq!(tree.len(), 3);
        let titles: Vec<&str> = tree.iter().map(|i| i.title.as_str()).collect();
        assert!(titles.contains(&"Child 1"));
        assert!(titles.contains(&"Child 2"));
        assert!(titles.contains(&"Grandchild"));
        // Ordered by created_at
        assert_eq!(tree[0].id, child1.id);
        assert_eq!(tree[1].id, child2.id);
        assert_eq!(tree[2].id, grandchild.id);
    }

    #[tokio::test]
    async fn get_issue_tree_empty() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Leaf".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let tree = get_issue_tree(&db, &issue.id).await.unwrap();
        assert!(tree.is_empty());
    }

    #[tokio::test]
    async fn insert_and_get_usage() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Usage test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        insert_usage(
            &db,
            &issue.id,
            &UsageStats {
                input_tokens: 1000,
                output_tokens: 500,
                model: Some("claude-sonnet".into()),
                cost_usd: Some(0.0105),
            },
        )
        .await
        .unwrap();

        let summary = get_usage_summary(&db, &issue.id).await.unwrap().unwrap();
        assert_eq!(summary.input_tokens, 1000);
        assert_eq!(summary.output_tokens, 500);
        assert_eq!(summary.attempts, 1);
        assert_eq!(summary.model.as_deref(), Some("claude-sonnet"));
    }

    #[tokio::test]
    async fn usage_accumulates_across_retries() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Retry usage".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        for _ in 0..3 {
            insert_usage(
                &db,
                &issue.id,
                &UsageStats {
                    input_tokens: 100,
                    output_tokens: 50,
                    model: Some("claude-sonnet".into()),
                    cost_usd: Some(0.001),
                },
            )
            .await
            .unwrap();
        }

        let summary = get_usage_summary(&db, &issue.id).await.unwrap().unwrap();
        assert_eq!(summary.input_tokens, 300);
        assert_eq!(summary.output_tokens, 150);
        assert_eq!(summary.attempts, 3);
    }

    #[test]
    fn estimate_cost_sonnet() {
        let cost = estimate_cost(1_000_000, 1_000_000, Some("claude-sonnet-4"));
        assert!((cost - 18.0).abs() < 0.01); // $3/M in + $15/M out
    }

    #[test]
    fn estimate_cost_opus() {
        let cost = estimate_cost(1_000_000, 1_000_000, Some("claude-opus-4"));
        assert!((cost - 90.0).abs() < 0.01); // $15/M in + $75/M out
    }

    #[tokio::test]
    async fn create_issue_rejects_duplicate_active_title() {
        let db = test_db().await;
        create_issue(
            &db,
            CreateIssueParams {
                title: "Unique task".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let err = create_issue(
            &db,
            CreateIssueParams {
                title: "Unique task".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("issue already exists"));
    }

    #[tokio::test]
    async fn create_issue_allows_duplicate_title_when_terminal() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Completable task".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let issue2 = create_issue(
            &db,
            CreateIssueParams {
                title: "Completable task".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_ne!(issue.id, issue2.id);
    }

    #[tokio::test]
    async fn create_issue_rejects_dependency_on_parent() {
        let db = test_db().await;
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "parent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let err = create_issue(
            &db,
            CreateIssueParams {
                title: "child".into(),
                parent_issue_id: Some(parent.id.clone()),
                dependencies: vec![parent.id.clone()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("ancestor"), "{err}");
    }

    #[tokio::test]
    async fn create_issue_rejects_dependency_on_grandparent() {
        let db = test_db().await;
        let grandparent = create_issue(
            &db,
            CreateIssueParams {
                title: "grandparent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "parent".into(),
                parent_issue_id: Some(grandparent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let err = create_issue(
            &db,
            CreateIssueParams {
                title: "child".into(),
                parent_issue_id: Some(parent.id.clone()),
                dependencies: vec![grandparent.id.clone()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("ancestor"), "{err}");
    }

    #[tokio::test]
    async fn create_issue_allows_dependency_on_sibling() {
        let db = test_db().await;
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "parent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let sibling = create_issue(
            &db,
            CreateIssueParams {
                title: "sibling".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Depending on a sibling is fine — no deadlock
        let child = create_issue(
            &db,
            CreateIssueParams {
                title: "child".into(),
                parent_issue_id: Some(parent.id.clone()),
                dependencies: vec![sibling.id.clone()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(child.dependencies, vec![sibling.id]);
    }

    #[tokio::test]
    async fn clear_events_removes_all_events() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Events test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        insert_event(
            &db,
            &issue.id,
            1,
            &crate::runner::StreamEvent::Text {
                text: "hello".into(),
            },
        )
        .await
        .unwrap();
        insert_event(
            &db,
            &issue.id,
            2,
            &crate::runner::StreamEvent::Text {
                text: "world".into(),
            },
        )
        .await
        .unwrap();

        let events = get_events_since(&db, &issue.id, 0).await.unwrap();
        assert_eq!(events.len(), 2);

        clear_events(&db, &issue.id).await.unwrap();

        let events = get_events_since(&db, &issue.id, 0).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn update_status_to_completed_sets_completed_at() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "Will complete".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(issue.completed_at.is_none());

        let updated = update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                status: Some(IssueStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(updated.completed_at.is_some());
    }

    #[tokio::test]
    async fn get_result_comment_returns_tagged_comment() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(get_result_comment(&db, &issue.id).await.unwrap().is_none());

        add_comment(&db, &issue.id, "agent", "the answer is 42", Some("result"))
            .await
            .unwrap();
        let result = get_result_comment(&db, &issue.id).await.unwrap();
        assert_eq!(result.as_deref(), Some("the answer is 42"));
    }

    #[tokio::test]
    async fn get_result_comments_batch_returns_map() {
        let db = test_db().await;
        let issue1 = create_issue(
            &db,
            CreateIssueParams {
                title: "batch1".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let issue2 = create_issue(
            &db,
            CreateIssueParams {
                title: "batch2".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        add_comment(&db, &issue1.id, "agent", "result1", Some("result"))
            .await
            .unwrap();
        add_comment(&db, &issue2.id, "agent", "result2", Some("result"))
            .await
            .unwrap();

        let ids = vec![issue1.id.clone(), issue2.id.clone()];
        let map: HashMap<String, String> = get_result_comments_batch(&db, &ids).await.unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&issue1.id).unwrap(), "result1");
        assert_eq!(map.get(&issue2.id).unwrap(), "result2");

        // Empty input returns empty map
        let empty_ids: Vec<String> = vec![];
        let empty: HashMap<String, String> = get_result_comments_batch(&db, &empty_ids).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn list_issues_children_of_scope() {
        let db = test_db().await;
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "Parent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child1 = create_issue(
            &db,
            CreateIssueParams {
                title: "Child 1".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child2 = create_issue(
            &db,
            CreateIssueParams {
                title: "Child 2".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Unrelated issue should not appear
        create_issue(
            &db,
            CreateIssueParams {
                title: "Unrelated".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = list_issues(
            &db,
            ListFilter {
                scope: IssueScope::ChildrenOf(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(result.issues.len(), 2);
        assert_eq!(result.total, 2);
        let ids: Vec<&str> = result.issues.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&child1.id.as_str()));
        assert!(ids.contains(&child2.id.as_str()));
    }

    #[tokio::test]
    async fn list_issues_top_level_interleaves_children() {
        let db = test_db().await;
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "Parent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        create_issue(
            &db,
            CreateIssueParams {
                title: "Child".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = list_issues(
            &db,
            ListFilter {
                scope: IssueScope::TopLevel,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Total counts only top-level issues
        assert_eq!(result.total, 1);
        // But the result list includes the child interleaved after its parent
        assert_eq!(result.issues.len(), 2);
        assert_eq!(result.issues[0].id, parent.id);
        assert!(result.issues[1].parent_issue_id.is_some());
    }

    #[tokio::test]
    async fn child_does_not_inherit_work_dir_by_default() {
        let db = test_db().await;
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "parent with worktree".into(),
                context: Some(serde_json::json!({"work_dir": "/some/path", "branch": "feat/x"})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child = create_issue(
            &db,
            CreateIssueParams {
                title: "child without same_worktree".into(),
                parent_issue_id: Some(parent.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(
            child.context.get("work_dir").is_none(),
            "child should not inherit work_dir; executor creates a new worktree at runtime"
        );
    }

    #[tokio::test]
    async fn child_inherits_work_dir_with_same_worktree() {
        let db = test_db().await;
        let parent = create_issue(
            &db,
            CreateIssueParams {
                title: "parent with worktree".into(),
                context: Some(serde_json::json!({"work_dir": "/some/path", "branch": "feat/x"})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let child = create_issue(
            &db,
            CreateIssueParams {
                title: "child with same_worktree".into(),
                parent_issue_id: Some(parent.id.clone()),
                context: Some(serde_json::json!({"same_worktree": true})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            child.context.get("work_dir").and_then(|v| v.as_str()),
            Some("/some/path"),
            "child with same_worktree should inherit parent's work_dir"
        );
    }

    #[tokio::test]
    async fn create_issue_rejects_empty_dependency_id() {
        let db = test_db().await;
        let dep = create_issue(
            &db,
            CreateIssueParams {
                title: "real dep".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let err = create_issue(
            &db,
            CreateIssueParams {
                title: "has empty dep".into(),
                dependencies: vec!["".into(), dep.id.clone()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("dependency ID must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn create_issue_rejects_nonexistent_dependency() {
        let db = test_db().await;
        let err = create_issue(
            &db,
            CreateIssueParams {
                title: "bad dep".into(),
                dependencies: vec!["nonexistent".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[tokio::test]
    async fn update_issue_rejects_nonexistent_dependency() {
        let db = test_db().await;
        let issue = create_issue(
            &db,
            CreateIssueParams {
                title: "will update".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let err = update_issue(
            &db,
            &issue.id,
            UpdateIssueParams {
                dependencies: Some(vec!["ghost".into()]),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }
}
