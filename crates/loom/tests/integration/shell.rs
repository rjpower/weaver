//! The operator scratch shell: `/api/shell/terminal` lazily spawns a single
//! login shell, keystrokes round-trip through it, and `POST /api/shell/restart`
//! replaces it with a fresh supervisor.

use std::time::Duration;

use futures_util::SinkExt;
use serde_json::json;
use serial_test::serial;
use tokio_tungstenite::tungstenite::Message;

use loom::backend;
use loom::shell::SHELL_SESSION;

use crate::fixtures::{drain_until, send_input, TermWs, TestServer};

async fn connect_shell(addr: &std::net::SocketAddr) -> TermWs {
    let url = format!("ws://{addr}/api/shell/terminal");
    let (ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .expect("shell websocket should connect");
    ws
}

/// Type an arithmetic marker until its *output* (distinct from the echoed
/// keystrokes) round-trips, proving the shell is up and executing.
async fn await_shell_ready(term: &mut TermWs, marker: &str) {
    let cmd = format!("echo {marker}$((6 * 7))\n");
    let want = format!("{marker}42");
    for _ in 0..15 {
        send_input(term, &cmd).await;
        if drain_until(term, &want, Duration::from_secs(1))
            .await
            .contains(&want)
        {
            return;
        }
    }
    panic!("scratch shell never came up");
}

#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_spawns_lazily_runs_and_restarts() {
    let ts = TestServer::start().await;

    // No shell exists until the first attach.
    assert!(
        !backend::has_session(SHELL_SESSION).await,
        "shell must not exist before anyone connects"
    );

    // Connecting spawns it; a command runs end to end.
    let mut term = connect_shell(&ts.addr).await;
    await_shell_ready(&mut term, "RDY").await;
    assert!(
        backend::has_session(SHELL_SESSION).await,
        "attaching must spawn the shell supervisor"
    );

    send_input(&mut term, "echo SHELL_MARKER_123\n").await;
    let out = drain_until(&mut term, "SHELL_MARKER_123", Duration::from_secs(8)).await;
    assert!(
        out.contains("SHELL_MARKER_123"),
        "marker never appeared in shell output:\n{out}"
    );
    term.send(Message::Close(None)).await.ok();
    drop(term);

    // Reconnecting reattaches to the SAME shell (closing a socket only detaches).
    let mut alive = false;
    for _ in 0..20 {
        if backend::has_session(SHELL_SESSION).await {
            alive = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(alive, "closing the socket must not kill the shell");

    // Restart replaces the supervisor; a fresh attach still works.
    ts.client
        .post("/api/shell/restart", json!({}))
        .await
        .unwrap();
    let mut term2 = connect_shell(&ts.addr).await;
    await_shell_ready(&mut term2, "RDY2").await;
    send_input(&mut term2, "echo SHELL_MARKER_456\n").await;
    let out2 = drain_until(&mut term2, "SHELL_MARKER_456", Duration::from_secs(8)).await;
    assert!(
        out2.contains("SHELL_MARKER_456"),
        "restarted shell never echoed:\n{out2}"
    );
    term2.send(Message::Close(None)).await.ok();

    // Best-effort cleanup so the detached supervisor doesn't linger.
    backend::kill_session(SHELL_SESSION).await.ok();
}
