use crate::db::Db;

pub struct SettingDef {
    pub key: &'static str,
    pub description: &'static str,
    pub default: &'static str,
}

pub static KNOWN_SETTINGS: &[SettingDef] = &[
    SettingDef {
        key: "executor.timeout_secs",
        description: "Agent execution timeout in seconds",
        default: "7200",
    },
    SettingDef {
        key: "executor.max_concurrent",
        description: "Maximum concurrent agents",
        default: "8",
    },
    SettingDef {
        key: "worktree.keep_count",
        description: "Number of recent terminal worktrees to keep during GC",
        default: "32",
    },
    SettingDef {
        key: "notify.slack.url",
        description: "Slack webhook URL for notifications",
        default: "",
    },
    SettingDef {
        key: "notify.discord.url",
        description: "Discord webhook URL for notifications",
        default: "",
    },
    SettingDef {
        key: "notify.generic.url",
        description: "Generic webhook URL for notifications",
        default: "",
    },
];

/// Look up the registered default for a known setting key.
pub fn known_default(key: &str) -> Option<&'static str> {
    KNOWN_SETTINGS.iter().find(|s| s.key == key).map(|s| s.default)
}

pub async fn get(db: &Db, key: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(v,)| v))
}

pub async fn get_or(db: &Db, key: &str, default: &str) -> anyhow::Result<String> {
    Ok(get(db, key).await?.unwrap_or_else(|| default.to_string()))
}

/// Get a known setting's value from DB, falling back to the registry default.
/// Errors if the key is not in `KNOWN_SETTINGS`.
pub async fn get_known(db: &Db, key: &str) -> anyhow::Result<String> {
    let default = known_default(key)
        .ok_or_else(|| anyhow::anyhow!("unknown setting: {key}"))?;
    get_or(db, key, default).await
}

pub async fn set(db: &Db, key: &str, value: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) \
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

/// Returns all settings as (key, value, updated_at) tuples.
pub async fn get_all(db: &Db) -> anyhow::Result<Vec<(String, String, String)>> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT key, value, updated_at FROM settings ORDER BY key")
            .fetch_all(db)
            .await?;
    Ok(rows)
}

/// Deletes a setting by key. Returns true if a row was actually deleted.
pub async fn delete(db: &Db, key: &str) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(db)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_db() -> Db {
        db::connect_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_key() {
        let db = test_db().await;
        assert_eq!(get(&db, "nonexistent").await.unwrap(), None);
    }

    #[tokio::test]
    async fn set_and_get_roundtrip() {
        let db = test_db().await;
        set(&db, "my.key", "hello").await.unwrap();
        assert_eq!(get(&db, "my.key").await.unwrap(), Some("hello".into()));
    }

    #[tokio::test]
    async fn set_overwrites_existing() {
        let db = test_db().await;
        set(&db, "k", "v1").await.unwrap();
        set(&db, "k", "v2").await.unwrap();
        assert_eq!(get(&db, "k").await.unwrap(), Some("v2".into()));
    }

    #[tokio::test]
    async fn get_or_returns_default() {
        let db = test_db().await;
        assert_eq!(get_or(&db, "missing", "fallback").await.unwrap(), "fallback");
    }

    #[tokio::test]
    async fn get_or_returns_stored_value() {
        let db = test_db().await;
        set(&db, "k", "stored").await.unwrap();
        assert_eq!(get_or(&db, "k", "fallback").await.unwrap(), "stored");
    }

    #[tokio::test]
    async fn get_known_returns_registry_default() {
        let db = test_db().await;
        let val = get_known(&db, "worktree.keep_count").await.unwrap();
        assert_eq!(val, "32");
    }

    #[tokio::test]
    async fn get_known_returns_stored_over_default() {
        let db = test_db().await;
        set(&db, "worktree.keep_count", "64").await.unwrap();
        let val = get_known(&db, "worktree.keep_count").await.unwrap();
        assert_eq!(val, "64");
    }

    #[tokio::test]
    async fn get_known_errors_on_unknown_key() {
        let db = test_db().await;
        assert!(get_known(&db, "totally.unknown").await.is_err());
    }

    #[tokio::test]
    async fn delete_returns_false_for_missing() {
        let db = test_db().await;
        assert!(!delete(&db, "nope").await.unwrap());
    }

    #[tokio::test]
    async fn delete_removes_existing() {
        let db = test_db().await;
        set(&db, "k", "v").await.unwrap();
        assert!(delete(&db, "k").await.unwrap());
        assert_eq!(get(&db, "k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn get_all_returns_all_settings() {
        let db = test_db().await;
        set(&db, "b.key", "b_val").await.unwrap();
        set(&db, "a.key", "a_val").await.unwrap();

        let all = get_all(&db).await.unwrap();
        assert_eq!(all.len(), 2);
        // Ordered by key
        assert_eq!(all[0].0, "a.key");
        assert_eq!(all[0].1, "a_val");
        assert_eq!(all[1].0, "b.key");
        assert_eq!(all[1].1, "b_val");
        // updated_at should be populated
        assert!(!all[0].2.is_empty());
    }
}
