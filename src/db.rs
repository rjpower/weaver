use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub type Db = SqlitePool;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS issues (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',

    prompt TEXT,
    context TEXT NOT NULL DEFAULT '{}',

    dependencies TEXT NOT NULL DEFAULT '[]',

    num_tries INTEGER NOT NULL DEFAULT 0,
    max_tries INTEGER NOT NULL DEFAULT 3,
    parent_issue_id TEXT,

    tags TEXT NOT NULL DEFAULT '[]',
    priority INTEGER NOT NULL DEFAULT 0,

    channel_kind TEXT,
    origin_ref TEXT,
    user_id TEXT,

    result TEXT,
    error TEXT,

    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_issues_status ON issues(status);
CREATE INDEX IF NOT EXISTS idx_issues_parent ON issues(parent_issue_id);
CREATE INDEX IF NOT EXISTS idx_issues_created ON issues(created_at);

CREATE TABLE IF NOT EXISTS issue_comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    issue_id TEXT NOT NULL REFERENCES issues(id),
    author TEXT NOT NULL DEFAULT 'system',
    body TEXT NOT NULL,
    tag TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_issue_comments_issue ON issue_comments(issue_id);
"#;

/// Default database path: searches for `.weaver/db.sqlite` walking up from the current directory.
/// If an existing database is found in a parent directory, that path is returned.
/// Otherwise, falls back to `.weaver/db.sqlite` in the current directory.
pub fn default_db_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(".weaver/db.sqlite");
        if candidate.exists() {
            return candidate;
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    cwd.join(".weaver/db.sqlite")
}

pub async fn connect(path: &Path) -> anyhow::Result<Db> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create database directory \"{}\"", parent.display())
        })?;
    }

    let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .with_context(|| format!("invalid database path \"{}\"", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .with_context(|| format!("failed to connect to database \"{}\"", path.display()))?;

    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn connect_in_memory() -> anyhow::Result<Db> {
    let options = SqliteConnectOptions::new()
        .in_memory(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

const MIGRATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS issue_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    issue_id TEXT NOT NULL REFERENCES issues(id),
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    model TEXT,
    cost_usd REAL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_issue_usage_issue ON issue_usage(issue_id);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

ALTER TABLE issue_comments ADD COLUMN tag TEXT;

CREATE TABLE IF NOT EXISTS issue_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    issue_id TEXT NOT NULL REFERENCES issues(id),
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    data TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_issue_events_issue_seq ON issue_events(issue_id, seq);

ALTER TABLE issues ADD COLUMN claude_session_id TEXT;
"#;

async fn run_migrations(pool: &Db) -> anyhow::Result<()> {
    for statement in SCHEMA.split(';') {
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            continue;
        }
        sqlx::query(trimmed).execute(pool).await.ok();
    }
    for statement in MIGRATIONS.split(';') {
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            continue;
        }
        sqlx::query(trimmed).execute(pool).await.ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_in_memory_runs_migrations() {
        let db = connect_in_memory().await.unwrap();
        // Should be able to insert an issue
        sqlx::query("INSERT INTO issues (id, title) VALUES ('test', 'Test issue')")
            .execute(&db)
            .await
            .unwrap();

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM issues")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn comments_table_exists() {
        let db = connect_in_memory().await.unwrap();
        sqlx::query("INSERT INTO issues (id, title) VALUES ('i1', 'Issue 1')")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO issue_comments (issue_id, author, body) VALUES ('i1', 'test', 'A comment')")
            .execute(&db)
            .await
            .unwrap();

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM issue_comments")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
}
