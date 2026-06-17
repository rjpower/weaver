//! `GET /api/sessions/{id}/conversation` — the normalized iris log behind the
//! dashboard's Conversation tab.

use serde_json::json;
use serial_test::serial;

use crate::fixtures::{plant_claude_transcript, HomeGuard, TestServer};

/// With a transcript present, the endpoint returns the parsed iris log: source,
/// model, and the user/assistant turns the viewer renders.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conversation_endpoint_returns_the_iris_log() {
    let ts = TestServer::start().await;
    let client = &ts.client;

    let home = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(home.path());

    let sess = client
        .post(
            "/api/sessions",
            json!({ "goal": "chat me", "cwd": ts.cwd(), "agent": "shell" }),
        )
        .await
        .unwrap();
    let id = sess["id"].as_str().unwrap().to_string();
    let work_dir = sess["work_dir"].as_str().unwrap().to_string();

    // Before any transcript exists, the endpoint 404s (no conversation yet).
    assert!(
        client
            .get(&format!("/api/sessions/{id}/conversation"))
            .await
            .is_err(),
        "no transcript yet → 404"
    );

    plant_claude_transcript(home.path(), &work_dir, "do the work", "Working on it.");

    let log = client
        .get(&format!("/api/sessions/{id}/conversation"))
        .await
        .unwrap();
    assert_eq!(log["source"], "claude");
    assert_eq!(log["model"], "claude-opus-4-8");
    let messages = log["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["blocks"][0]["kind"], "text");
    assert_eq!(messages[0]["blocks"][0]["text"], "do the work");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["blocks"][0]["text"], "Working on it.");
}
