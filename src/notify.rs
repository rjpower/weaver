use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::issue::Issue;
use crate::settings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "snake_case")]
pub enum NotifySink {
    Slack { url: String },
    Discord { url: String },
    Generic { url: String },
}

pub struct Notifier {
    sinks: Vec<NotifySink>,
}

impl Notifier {
    /// Build a Notifier from settings keys matching `notify.{service}.url`.
    pub async fn from_db(db: &Db) -> Self {
        let mut sinks = Vec::new();
        let all = settings::get_all(db).await.unwrap_or_default();
        for (key, value, _) in all {
            if let Some(sink) = parse_notify_key(&key, &value) {
                sinks.push(sink);
            }
        }
        Self { sinks }
    }

    /// Send a notification for the given event to all configured sinks concurrently.
    pub async fn notify(&self, event: &str, issue: &Issue) {
        for sink in &self.sinks {
            let event = event.to_string();
            let issue = issue.clone();
            let sink = sink.clone();
            tokio::spawn(async move {
                if let Err(e) = send_to_sink(&sink, &event, &issue).await {
                    tracing::warn!(error = %e, "Notification delivery failed");
                }
            });
        }
    }
}

fn parse_notify_key(key: &str, value: &str) -> Option<NotifySink> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() != 3 || parts[0] != "notify" || parts[2] != "url" {
        return None;
    }
    let service = parts[1];
    let url = value.to_string();
    match service {
        "slack" => Some(NotifySink::Slack { url }),
        "discord" => Some(NotifySink::Discord { url }),
        "generic" => Some(NotifySink::Generic { url }),
        _ => None,
    }
}

pub fn format_slack(event: &str, issue: &Issue) -> serde_json::Value {
    let emoji = match event {
        "issue.completed" => ":white_check_mark:",
        "issue.failed" => ":x:",
        "issue.awaiting_review" => ":eyes:",
        _ => ":bell:",
    };
    serde_json::json!({
        "text": format!("{emoji} *{event}*: `{}` {}", issue.id, issue.title),
    })
}

pub fn format_discord(event: &str, issue: &Issue) -> serde_json::Value {
    let color = match event {
        "issue.completed" => 0x22c55e_i64, // green
        "issue.failed" => 0xef4444_i64,    // red
        "issue.awaiting_review" => 0xeab308_i64, // yellow
        _ => 0x6b7280_i64,                 // gray
    };
    serde_json::json!({
        "content": format!("**{event}**: `{}` {}", issue.id, issue.title),
        "embeds": [{
            "title": issue.title,
            "description": format!("`{}` — {}", issue.id, issue.status),
            "color": color,
            "fields": [
                { "name": "Status", "value": issue.status.to_string(), "inline": true },
                { "name": "Tags", "value": issue.tags.join(", "), "inline": true },
            ],
        }],
    })
}

pub fn format_generic(event: &str, issue: &Issue) -> serde_json::Value {
    serde_json::json!({
        "event": event,
        "issue": {
            "id": issue.id,
            "title": issue.title,
            "status": issue.status.to_string(),
            "error": issue.error,
            "tags": issue.tags,
            "created_at": issue.created_at,
            "completed_at": issue.completed_at,
        }
    })
}

async fn send_to_sink(sink: &NotifySink, event: &str, issue: &Issue) -> anyhow::Result<()> {
    let (url, payload) = match sink {
        NotifySink::Slack { url } => (url.as_str(), format_slack(event, issue)),
        NotifySink::Discord { url } => (url.as_str(), format_discord(event, issue)),
        NotifySink::Generic { url } => (url.as_str(), format_generic(event, issue)),
    };

    let body = serde_json::to_string(&payload)?;
    let result = tokio::process::Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            url,
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
            "--max-time",
            "10",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match result {
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(url, %stderr, "Notification delivery failed: exit {}", out.status);
        }
        Err(e) => {
            tracing::warn!(url, error = %e, "Notification delivery failed");
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue::IssueStatus;

    fn test_issue() -> Issue {
        Issue {
            id: "abcdef12-3456-7890-abcd-ef1234567890".into(),
            title: "Fix login bug".into(),
            body: String::new(),
            status: IssueStatus::Completed,
            context: serde_json::json!({}),
            dependencies: vec![],
            num_tries: 1,
            max_tries: 3,
            parent_issue_id: None,
            tags: vec!["bug".into()],
            priority: 0,
            channel_kind: None,
            origin_ref: None,
            user_id: None,
            error: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:01.000Z".into(),
            completed_at: Some("2026-01-01T00:00:01.000Z".into()),
            claude_session_id: None,
        }
    }

    #[test]
    fn slack_payload_completed() {
        let issue = test_issue();
        let payload = format_slack("issue.completed", &issue);
        let text = payload["text"].as_str().unwrap();
        assert!(text.contains(":white_check_mark:"));
        assert!(text.contains("issue.completed"));
        assert!(text.contains("abcdef12"));
        assert!(text.contains("Fix login bug"));
    }

    #[test]
    fn slack_payload_failed_emoji() {
        let mut issue = test_issue();
        issue.status = IssueStatus::Failed;
        let payload = format_slack("issue.failed", &issue);
        let text = payload["text"].as_str().unwrap();
        assert!(text.contains(":x:"));
    }

    #[test]
    fn slack_payload_awaiting_review_emoji() {
        let issue = test_issue();
        let payload = format_slack("issue.awaiting_review", &issue);
        let text = payload["text"].as_str().unwrap();
        assert!(text.contains(":eyes:"));
    }

    #[test]
    fn discord_payload_structure() {
        let issue = test_issue();
        let payload = format_discord("issue.completed", &issue);
        assert!(payload["content"].as_str().unwrap().contains("issue.completed"));
        assert!(payload["content"].as_str().unwrap().contains("abcdef12"));

        let embeds = payload["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0]["title"], "Fix login bug");
        // green for completed
        assert_eq!(embeds[0]["color"], 0x22c55e);
    }

    #[test]
    fn discord_payload_red_for_failed() {
        let mut issue = test_issue();
        issue.status = IssueStatus::Failed;
        let payload = format_discord("issue.failed", &issue);
        assert_eq!(payload["embeds"][0]["color"], 0xef4444);
    }

    #[test]
    fn discord_payload_yellow_for_review() {
        let issue = test_issue();
        let payload = format_discord("issue.awaiting_review", &issue);
        assert_eq!(payload["embeds"][0]["color"], 0xeab308);
    }

    #[test]
    fn generic_payload_structure() {
        let issue = test_issue();
        let payload = format_generic("issue.completed", &issue);
        assert_eq!(payload["event"], "issue.completed");
        assert_eq!(payload["issue"]["id"], issue.id);
        assert_eq!(payload["issue"]["title"], "Fix login bug");
        assert_eq!(payload["issue"]["status"], "completed");
        assert_eq!(payload["issue"]["tags"][0], "bug");
        assert!(payload["issue"]["created_at"].is_string());
        assert!(payload["issue"]["completed_at"].is_string());
    }

    #[test]
    fn generic_payload_includes_error() {
        let mut issue = test_issue();
        issue.status = IssueStatus::Failed;
        issue.error = Some("timeout after 30s".into());
        let payload = format_generic("issue.failed", &issue);
        assert_eq!(payload["issue"]["error"], "timeout after 30s");
    }

    #[test]
    fn parse_notify_key_slack() {
        let sink = parse_notify_key("notify.slack.url", "https://hooks.slack.com/foo");
        assert!(matches!(sink, Some(NotifySink::Slack { url }) if url == "https://hooks.slack.com/foo"));
    }

    #[test]
    fn parse_notify_key_discord() {
        let sink = parse_notify_key("notify.discord.url", "https://discord.com/hook");
        assert!(matches!(sink, Some(NotifySink::Discord { url }) if url == "https://discord.com/hook"));
    }

    #[test]
    fn parse_notify_key_generic() {
        let sink = parse_notify_key("notify.generic.url", "https://example.com/hook");
        assert!(matches!(sink, Some(NotifySink::Generic { url }) if url == "https://example.com/hook"));
    }

    #[test]
    fn parse_notify_key_ignores_non_notify() {
        assert!(parse_notify_key("executor.timeout_secs", "3600").is_none());
    }

    #[test]
    fn parse_notify_key_ignores_unknown_service() {
        assert!(parse_notify_key("notify.telegram.url", "https://t.me/hook").is_none());
    }

    #[tokio::test]
    async fn notifier_from_db_builds_sinks() {
        let db = crate::db::connect_in_memory().await.unwrap();
        crate::settings::set(&db, "notify.slack.url", "https://hooks.slack.com/test")
            .await
            .unwrap();
        crate::settings::set(&db, "notify.generic.url", "https://example.com/hook")
            .await
            .unwrap();

        let notifier = Notifier::from_db(&db).await;
        assert_eq!(notifier.sinks.len(), 2);
    }
}
