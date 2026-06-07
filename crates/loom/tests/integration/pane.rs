//! Driving a session's tmux pane over REST: `send` types + submits a line,
//! `preview` reads the pane back, `interrupt` injects a break, and all three
//! refuse a session whose tmux is gone. These use `tmux send-keys`/`capture-pane`
//! (no PTY), so unlike the terminal WebSocket suite they run everywhere.

use std::time::Duration;

use serde_json::json;
use serial_test::serial;

use loom::tmux;

use crate::fixtures::TestServer;

/// Poll `GET /preview` until the captured screen contains `marker`, or fail.
async fn preview_until(ts: &TestServer, id: &str, marker: &str) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let res = ts
            .client
            .get(&format!("/api/sessions/{id}/preview"))
            .await
            .unwrap();
        let screen = res["screen"].as_str().unwrap_or("").to_string();
        if screen.contains(marker) {
            return screen;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("marker {marker:?} never appeared in the pane; last capture:\n{screen}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `send` (submit) runs a command in the shell; `preview` reads its output back.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_runs_a_command_and_preview_reads_it() {
    let ts = TestServer::start().await;
    let client = &ts.client;

    let ws = client
        .post(
            "/api/sessions",
            json!({ "goal": "pane test", "cwd": ts.cwd(), "agent": "shell" }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();

    // Submit a command whose OUTPUT (the arithmetic result) differs from the
    // text typed — so finding it proves the line was actually executed, not just
    // echoed onto the prompt.
    let sent = client
        .post(
            &format!("/api/sessions/{id}/send"),
            json!({ "text": "echo PANE_$((6 * 7))" }),
        )
        .await
        .unwrap();
    assert_eq!(sent["submitted"], true, "submit defaults to true");

    let screen = preview_until(&ts, &id, "PANE_42").await;
    assert!(screen.contains("PANE_42"), "command output missing");

    client.delete(&format!("/api/sessions/{id}")).await.unwrap();
}

/// `send` with `submit:false` stages input without running it.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_without_submit_does_not_execute() {
    let ts = TestServer::start().await;
    let client = &ts.client;

    let ws = client
        .post(
            "/api/sessions",
            json!({ "goal": "pane test", "cwd": ts.cwd(), "agent": "shell" }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();

    let sent = client
        .post(
            &format!("/api/sessions/{id}/send"),
            json!({ "text": "echo STAGED_$((1 + 1))", "submit": false }),
        )
        .await
        .unwrap();
    assert_eq!(sent["submitted"], false);

    // Give the pane a beat, then confirm the arithmetic never ran: the literal
    // text is on the prompt line, but the evaluated `STAGED_2` is not.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let res = client
        .get(&format!("/api/sessions/{id}/preview"))
        .await
        .unwrap();
    let screen = res["screen"].as_str().unwrap_or("");
    assert!(
        !screen.contains("STAGED_2"),
        "unsubmitted input should not have executed; screen:\n{screen}"
    );

    client.delete(&format!("/api/sessions/{id}")).await.unwrap();
}

/// `interrupt` injects an Escape and reports success.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupt_sends_a_break() {
    let ts = TestServer::start().await;
    let client = &ts.client;

    let ws = client
        .post(
            "/api/sessions",
            json!({ "goal": "pane test", "cwd": ts.cwd(), "agent": "shell" }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();

    let res = client
        .post(&format!("/api/sessions/{id}/interrupt"), json!({}))
        .await
        .unwrap();
    assert_eq!(res["interrupted"], true);

    client.delete(&format!("/api/sessions/{id}")).await.unwrap();
}

/// All three pane endpoints 409 when the session has no live tmux.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pane_endpoints_reject_a_dead_session() {
    let ts = TestServer::start().await;
    let client = &ts.client;

    let ws = client
        .post(
            "/api/sessions",
            json!({ "goal": "pane test", "cwd": ts.cwd(), "agent": "shell" }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();
    let session = ws["tmux_session"].as_str().unwrap().to_string();

    // Kill the tmux out from under loom — the session is now orphaned.
    tmux::kill_session(&session).await.unwrap();
    assert!(!tmux::has_session(&session).await);

    assert!(
        client
            .post(
                &format!("/api/sessions/{id}/send"),
                json!({ "text": "echo hi" })
            )
            .await
            .is_err(),
        "send should fail without a live tmux"
    );
    assert!(
        client
            .post(&format!("/api/sessions/{id}/interrupt"), json!({}))
            .await
            .is_err(),
        "interrupt should fail without a live tmux"
    );
    assert!(
        client
            .get(&format!("/api/sessions/{id}/preview"))
            .await
            .is_err(),
        "preview should fail without a live tmux"
    );

    client.delete(&format!("/api/sessions/{id}")).await.unwrap();
}
