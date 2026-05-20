//! Key/value settings stored in the `settings` table.

use anyhow::Result;
use sqlx::Row;

use crate::db::Db;

pub const DEFAULT_AGENT: &str = "claude";
pub const DEFAULT_SUMMARY_INTERVAL_SECS: i64 = 600;
/// Whether the server adopts orphaned workspaces on startup. Off by default:
/// the operator opts in via `weaver config set server.auto_adopt true`.
pub const DEFAULT_AUTO_ADOPT: bool = false;

pub async fn get(db: &Db, key: &str) -> Option<String> {
    let value = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|r| r.get::<String, _>("value"));
    tracing::debug!(key, found = value.is_some(), "config get");
    value
}

pub async fn get_or(db: &Db, key: &str, default: &str) -> String {
    get(db, key).await.unwrap_or_else(|| default.to_string())
}

pub async fn get_i64(db: &Db, key: &str, default: i64) -> i64 {
    get(db, key)
        .await
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Read a boolean setting. Accepts `true`/`1`/`yes`/`on` (case-insensitively)
/// as true and `false`/`0`/`no`/`off` as false; anything else falls back to
/// `default`.
pub async fn get_bool(db: &Db, key: &str, default: bool) -> bool {
    match get(db, key).await {
        Some(v) => match v.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            _ => default,
        },
        None => default,
    }
}

pub async fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    tracing::debug!(key, value, "config set");
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn delete(db: &Db, key: &str) -> Result<()> {
    tracing::debug!(key, "config delete");
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn list(db: &Db) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query("SELECT key, value FROM settings ORDER BY key")
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get::<String, _>("key"), r.get::<String, _>("value")))
        .collect())
}
