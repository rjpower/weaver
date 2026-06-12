//! The terminal WebSocket: keystrokes reach the PTY and output round-trips, a
//! resize propagates, a large burst survives backpressure, and disconnecting
//! kills only the attach client — not the session itself.

use std::time::Duration;

use futures_util::SinkExt;
use serde_json::json;
use serial_test::serial;
use tokio_tungstenite::tungstenite::Message;

use loom::backend;

use crate::fixtures::{connect_terminal, drain_until, resize_frame, send_input, TestServer};

#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_websocket_roundtrip() {
    let ts = TestServer::start().await;
    let client = &ts.client;

    let ws = client
        .post(
            "/api/sessions",
            json!({
                "goal": "terminal test",
                "cwd": ts.cwd(),
                "agent": "shell",
            }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();
    let session = ws["term_session"].as_str().unwrap().to_string();

    let mut term = connect_terminal(&ts.addr, &id).await;

    // Wait for the shell to come up before asserting anything: the launch script
    // `exec`s the shell only after the supervisor socket is already listening, and
    // shell startup flushes input typed during that window — so an early command
    // can be echoed but never run. Re-send an arithmetic marker until its OUTPUT
    // (`RDY42`, distinct from the typed text) round-trips, proving the shell is
    // executing and the WS path works end to end.
    let mut ready = false;
    for _ in 0..15 {
        send_input(&mut term, "echo RDY$((6 * 7))\n").await;
        if drain_until(&mut term, "RDY42", Duration::from_secs(1))
            .await
            .contains("RDY42")
        {
            ready = true;
            break;
        }
    }
    assert!(ready, "shell never came up");

    // Drive a real size, give the supervisor a moment to apply the SIGWINCH, then
    // run a command that prints the terminal width. The 0x01 → master.resize →
    // SIGWINCH path must reach the shell.
    term.send(Message::Binary(resize_frame(120, 40).into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    send_input(&mut term, "echo DIMS COLS=$(tput cols)\n").await;
    let dims = drain_until(&mut term, "COLS=120", Duration::from_secs(10)).await;
    assert!(
        dims.contains("COLS=120"),
        "resize did not propagate to the pty width; got:\n{dims}"
    );

    // Output round-trip (the echoed keystrokes alone prove input→PTY→output).
    send_input(&mut term, "echo WS_MARKER_123\n").await;
    let out = drain_until(&mut term, "WS_MARKER_123", Duration::from_secs(8)).await;
    assert!(
        out.contains("WS_MARKER_123"),
        "marker never appeared in terminal output:\n{out}"
    );

    // Backpressure: a large burst must arrive without truncation/deadlock.
    // "line_5000" appears only in the OUTPUT, never in the typed command.
    send_input(&mut term, "for i in $(seq 1 5000); do echo line_$i; done\n").await;
    let burst = drain_until(&mut term, "line_5000", Duration::from_secs(20)).await;
    assert!(
        burst.contains("line_1") && burst.contains("line_5000"),
        "burst output was truncated under backpressure"
    );

    // Closing the socket must detach only this attach client; the supervisor and
    // its child keep running.
    term.send(Message::Close(None)).await.ok();
    drop(term);
    let mut alive = true;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        alive = backend::has_session(&session).await;
        if !alive {
            break;
        }
    }
    assert!(alive, "closing the terminal must not kill the session");

    // A second connection still works — proves attach-client-only teardown.
    let mut term2 = connect_terminal(&ts.addr, &id).await;
    send_input(&mut term2, "echo WS_MARKER_456\n").await;
    let out2 = drain_until(&mut term2, "WS_MARKER_456", Duration::from_secs(8)).await;
    assert!(
        out2.contains("WS_MARKER_456"),
        "reconnected terminal never echoed:\n{out2}"
    );
    term2.send(Message::Close(None)).await.ok();

    client.delete(&format!("/api/sessions/{id}")).await.unwrap();
}
