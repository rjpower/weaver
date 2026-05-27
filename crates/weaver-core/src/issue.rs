//! Per-branch issue tracker: the agent's todo list for a workstream.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::db::{now_iso, Db};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Issue {
    pub id: i64,
    pub branch_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub github_issue: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
}

/// Create a new issue. Returns the persisted row.
pub async fn add(
    db: &Db,
    branch_id: &str,
    title: &str,
    body: &str,
    github_issue: Option<i64>,
) -> Result<Issue> {
    let now = now_iso();
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO issues (branch_id, title, body, status, github_issue, created_at, updated_at)
         VALUES (?, ?, ?, 'open', ?, ?, ?) RETURNING id",
    )
    .bind(branch_id)
    .bind(title)
    .bind(body)
    .bind(github_issue)
    .bind(&now)
    .bind(&now)
    .fetch_one(db)
    .await?;
    get(db, row.0)
        .await?
        .ok_or_else(|| anyhow::anyhow!("issue vanished after insert"))
}

pub async fn get(db: &Db, id: i64) -> Result<Option<Issue>> {
    let row = sqlx::query_as::<_, Issue>("SELECT * FROM issues WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row)
}

/// All issues for a branch. `include_closed` selects open vs all.
pub async fn list_for_branch(
    db: &Db,
    branch_id: &str,
    include_closed: bool,
) -> Result<Vec<Issue>> {
    let sql = if include_closed {
        "SELECT * FROM issues WHERE branch_id = ? ORDER BY id ASC"
    } else {
        "SELECT * FROM issues WHERE branch_id = ? AND status = 'open' ORDER BY id ASC"
    };
    let rows = sqlx::query_as::<_, Issue>(sql)
        .bind(branch_id)
        .fetch_all(db)
        .await?;
    Ok(rows)
}

/// Count of open issues on a branch.
pub async fn open_count(db: &Db, branch_id: &str) -> Result<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM issues WHERE branch_id = ? AND status = 'open'",
    )
    .bind(branch_id)
    .fetch_one(db)
    .await?;
    Ok(n)
}

pub async fn close(db: &Db, id: i64) -> Result<()> {
    let now = now_iso();
    sqlx::query(
        "UPDATE issues SET status = 'closed', closed_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn reopen(db: &Db, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE issues SET status = 'open', closed_at = NULL, updated_at = ? WHERE id = ?",
    )
    .bind(now_iso())
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete(db: &Db, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM issues WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch;

    #[tokio::test]
    async fn lifecycle() {
        let db = crate::db::connect_in_memory().await.unwrap();
        let b = branch::upsert(&db, "/r", "main", "main").await.unwrap();
        let i = add(&db, &b.id, "fix the thing", "", None).await.unwrap();
        assert_eq!(i.status, "open");

        let open = list_for_branch(&db, &b.id, false).await.unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open_count(&db, &b.id).await.unwrap(), 1);

        close(&db, i.id).await.unwrap();
        let open = list_for_branch(&db, &b.id, false).await.unwrap();
        assert_eq!(open.len(), 0);
        let all = list_for_branch(&db, &b.id, true).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, "closed");

        reopen(&db, i.id).await.unwrap();
        assert_eq!(open_count(&db, &b.id).await.unwrap(), 1);

        delete(&db, i.id).await.unwrap();
        assert_eq!(list_for_branch(&db, &b.id, true).await.unwrap().len(), 0);
    }
}
