//! The loom ACP client end to end: a real relay supervisor runs the scripted
//! `fake-acp-agent.mjs`, and `loom::acp` drives it over JSON-RPC while the HTTP
//! `/chat`, `/prompt`, `/permissions`, `/mode`, and `/interrupt` routes exercise
//! the same session. The suite shares the server's `AppState` (its ACP registry
//! is `Arc`-shared), so `loom::acp::start`/`attach` register into the very
//! registry the routes read.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use serial_test::serial;
use tokio::sync::broadcast;

use loom::acp::{self, AcpLaunch, NewOrLoad, SseEvent};
use loom::session::{self as session_mod, NewSession};

use crate::fixtures::TestServer;

/// The relay command that launches the scripted fake ACP agent over stdio.
fn agent_cmd() -> String {
    format!(
        "node {}/tests/fixtures/fake-acp-agent.mjs",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Insert a fresh (branch, session) pair directly — the session row `acp::start`
/// binds a relay to. `term_session` doubles as the relay name.
async fn make_session(ts: &TestServer, id: &str) {
    let branch = loom::branch::upsert(&ts.state.db, &ts.cwd(), &format!("weaver/{id}"), "main")
        .await
        .unwrap();
    session_mod::insert(
        &ts.state.db,
        &NewSession {
            id: id.to_string(),
            branch_id: branch.id,
            work_dir: ts.cwd(),
            term_session: format!("weaver-{id}"),
            agent_kind: "claude".to_string(),
            model: String::new(),
            effort: String::new(),
            status: "running".to_string(),
            github_repo: None,
            parent_branch_id: None,
            managed_by: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
}

/// Bring up a fresh ACP session (relay + handshake + task) with the given launch
/// mode and optional goal.
async fn start_new(ts: &TestServer, id: &str, mode: Option<&str>, goal: Option<&str>) {
    make_session(ts, id).await;
    let cwd = ts.repo_path().to_path_buf();
    let launch = AcpLaunch {
        adapter_cmd: agent_cmd(),
        cwd: cwd.clone(),
        env: vec![],
        new_or_load: NewOrLoad::New { cwd, meta: None },
        mode: mode.map(str::to_string),
        goal: goal.map(str::to_string),
    };
    acp::start(&ts.state, id, launch)
        .await
        .expect("acp session starts");
}

/// Collect broadcast SSE events until `until` matches one (or the timeout).
async fn drain_events(
    rx: &mut broadcast::Receiver<SseEvent>,
    timeout: Duration,
    until: impl Fn(&SseEvent) -> bool,
) -> Vec<SseEvent> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut out = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                let stop = until(&ev);
                out.push(ev);
                if stop {
                    break;
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }
    out
}

/// Poll `GET /chat` until `pred` accepts the block list, returning the chat body.
async fn poll_chat(
    ts: &TestServer,
    id: &str,
    timeout: Duration,
    pred: impl Fn(&[Value]) -> bool,
) -> Value {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let chat = ts
            .client
            .get(&format!("/api/sessions/{id}/chat"))
            .await
            .unwrap();
        let empty = vec![];
        let blocks = chat["blocks"].as_array().unwrap_or(&empty);
        if pred(blocks) {
            return chat;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("chat never satisfied the predicate; last: {chat}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn kinds(blocks: &[Value]) -> Vec<String> {
    blocks
        .iter()
        .map(|b| b["kind"].as_str().unwrap_or("").to_string())
        .collect()
}

fn count_kind(blocks: &[Value], kind: &str) -> usize {
    blocks.iter().filter(|b| b["kind"] == kind).count()
}

/// 1. New session end to end: prompt → journal has user_message + agent_message +
///    turn_end; SSE delivered delta + block + turn events.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_session_end_to_end() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-e2e", None, None).await;

    // Subscribe before prompting so no event is missed.
    let mut rx = ts
        .state
        .acp
        .get("acp-e2e")
        .expect("task registered")
        .subscribe();

    let res = ts
        .client
        .post(
            "/api/sessions/acp-e2e/prompt",
            json!({ "text": "say:hello" }),
        )
        .await
        .unwrap();
    assert_eq!(res["queued"], false, "an idle session dispatches at once");
    assert_eq!(res["turn"], 0);

    let events = drain_events(&mut rx, Duration::from_secs(10), |e| {
        e.event == "turn" && e.data["state"] == "ended"
    })
    .await;

    assert!(
        events
            .iter()
            .any(|e| e.event == "turn" && e.data["state"] == "started"),
        "a turn-started event"
    );
    assert!(
        events
            .iter()
            .any(|e| e.event == "delta" && e.data["kind"] == "agent_message"),
        "an agent_message delta streamed"
    );
    assert!(
        events.iter().any(|e| e.event == "block"
            && e.data["kind"] == "agent_message"
            && e.data["payload"]["text"] == "hello"),
        "a consolidated agent_message block"
    );
    assert!(
        events
            .iter()
            .any(|e| e.event == "block" && e.data["kind"] == "user_message"),
        "the user_message block"
    );
    assert!(
        events.iter().any(|e| e.event == "turn"
            && e.data["state"] == "ended"
            && e.data["stop_reason"] == "end_turn"),
        "a turn-ended event with end_turn"
    );

    let chat = ts.client.get("/api/sessions/acp-e2e/chat").await.unwrap();
    let blocks = chat["blocks"].as_array().unwrap();
    let ks = kinds(blocks);
    assert!(ks.contains(&"user_message".to_string()));
    assert!(ks.contains(&"agent_message".to_string()));
    assert!(ks.contains(&"turn_end".to_string()));
    assert_eq!(
        chat["live_turn"],
        Value::Null,
        "turn ended, nothing in flight"
    );

    // The SessionView exposes the ACP fields.
    let view = ts.client.get("/api/sessions/acp-e2e").await.unwrap();
    assert_eq!(view["protocol"], "acp");
    assert!(view["acp_session_id"]
        .as_str()
        .unwrap()
        .starts_with("fake-session-"));
}

/// 1b. The HTTP `/chat/stream` route streams the same events over SSE.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_stream_route_delivers_sse() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-sse", None, None).await;

    // Opening the stream subscribes the broadcast before we prompt.
    let url = format!("http://{}/api/sessions/acp-sse/chat/stream", ts.addr);
    let resp = reqwest::Client::new().get(&url).send().await.unwrap();
    assert!(resp.status().is_success());

    ts.client
        .post(
            "/api/sessions/acp-sse/prompt",
            json!({ "text": "say:streamed" }),
        )
        .await
        .unwrap();

    let mut stream = resp.bytes_stream();
    let mut body = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                body.push_str(&String::from_utf8_lossy(&chunk));
                if body.contains("\"state\":\"ended\"") {
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(
        body.contains("event: turn"),
        "stream carried turn events: {body}"
    );
    assert!(
        body.contains("event: block"),
        "stream carried block events: {body}"
    );
    assert!(
        body.contains("event: delta"),
        "stream carried delta events: {body}"
    );
}

/// 2. Tool call: a live `tool` SSE, then one journaled `tool_call` block at a
///    terminal status carrying the diff content.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_call_live_then_journaled() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-tool", None, None).await;
    let mut rx = ts.state.acp.get("acp-tool").unwrap().subscribe();

    ts.client
        .post(
            "/api/sessions/acp-tool/prompt",
            json!({ "text": "tool:edit" }),
        )
        .await
        .unwrap();

    let events = drain_events(&mut rx, Duration::from_secs(10), |e| {
        e.event == "turn" && e.data["state"] == "ended"
    })
    .await;

    assert!(
        events.iter().any(|e| e.event == "tool"
            && e.data["status"] == "in_progress"
            && e.data["tool_kind"] == "edit"),
        "a live in-progress tool event"
    );
    let tool_blocks: Vec<&SseEvent> = events
        .iter()
        .filter(|e| e.event == "block" && e.data["kind"] == "tool_call")
        .collect();
    assert_eq!(
        tool_blocks.len(),
        1,
        "exactly one journaled tool_call block"
    );
    assert_eq!(tool_blocks[0].data["payload"]["status"], "completed");
    let content = tool_blocks[0].data["payload"]["content"]
        .as_array()
        .unwrap();
    assert!(
        content
            .iter()
            .any(|c| c["type"] == "diff" && c["new"] == "new line\n"),
        "the diff content survived: {content:?}"
    );

    let chat = ts.client.get("/api/sessions/acp-tool/chat").await.unwrap();
    let blocks = chat["blocks"].as_array().unwrap();
    assert_eq!(
        count_kind(blocks, "tool_call"),
        1,
        "one tool_call in the journal"
    );
}

/// 3a. Permission auto-answer: under `bypassPermissions` the request is answered
///     by policy and the turn completes without a REST call.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_auto_answered_under_bypass() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-auto", Some("bypassPermissions"), None).await;
    let mut rx = ts.state.acp.get("acp-auto").unwrap().subscribe();

    ts.client
        .post(
            "/api/sessions/acp-auto/prompt",
            json!({ "text": "permission:secret|say:done" }),
        )
        .await
        .unwrap();

    let events = drain_events(&mut rx, Duration::from_secs(10), |e| {
        e.event == "turn" && e.data["state"] == "ended"
    })
    .await;
    assert!(
        events.iter().any(|e| e.event == "turn"
            && e.data["state"] == "ended"
            && e.data["stop_reason"] == "end_turn"),
        "the turn completed after the auto-answer"
    );

    let chat = ts.client.get("/api/sessions/acp-auto/chat").await.unwrap();
    let blocks = chat["blocks"].as_array().unwrap();
    let perm = blocks
        .iter()
        .find(|b| b["kind"] == "permission_request")
        .expect("a permission_request block");
    assert_eq!(
        perm["payload"]["outcome"]["option_id"], "allow-once",
        "first allow chosen"
    );
    assert_eq!(
        perm["payload"]["outcome"]["by"], "policy",
        "answered by policy"
    );
    // The turn still reached `say:done`.
    assert!(
        blocks
            .iter()
            .any(|b| b["kind"] == "agent_message" && b["payload"]["text"] == "done"),
        "the turn continued past the permission"
    );
}

/// 3b. Permission REST-answer: under `default` the request stays open until a
///     `POST /permissions/{id}` answers it, then the turn completes.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_answered_over_rest() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-rest", Some("default"), None).await;

    ts.client
        .post(
            "/api/sessions/acp-rest/prompt",
            json!({ "text": "permission:edit-file|say:granted" }),
        )
        .await
        .unwrap();

    // The request surfaces as an open permission_request block (no auto-answer).
    let chat = poll_chat(&ts, "acp-rest", Duration::from_secs(10), |blocks| {
        blocks
            .iter()
            .any(|b| b["kind"] == "permission_request" && b["payload"]["outcome"].is_null())
    })
    .await;
    let request_id = chat["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "permission_request")
        .unwrap()["payload"]["request_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(chat["live_turn"], 0, "the turn is blocked, still in flight");

    // Answering an unknown id is a 404.
    assert!(
        ts.client
            .post(
                "/api/sessions/acp-rest/permissions/nope",
                json!({ "option_id": "allow-once" }),
            )
            .await
            .is_err(),
        "unknown request id 404s"
    );

    let res = ts
        .client
        .post(
            &format!("/api/sessions/acp-rest/permissions/{request_id}"),
            json!({ "option_id": "allow-once" }),
        )
        .await
        .unwrap();
    assert_eq!(res["resolved"], true);

    // The agent got the answer and the turn completed.
    let chat = poll_chat(&ts, "acp-rest", Duration::from_secs(10), |blocks| {
        blocks.iter().any(|b| b["kind"] == "turn_end")
    })
    .await;
    let blocks = chat["blocks"].as_array().unwrap();
    let perm = blocks
        .iter()
        .find(|b| b["kind"] == "permission_request")
        .unwrap();
    assert_eq!(perm["payload"]["outcome"]["option_id"], "allow-once");
    assert_eq!(perm["payload"]["outcome"]["by"], "manual");

    // Answering again is a 409 (already resolved).
    assert!(
        ts.client
            .post(
                &format!("/api/sessions/acp-rest/permissions/{request_id}"),
                json!({ "option_id": "allow-once" }),
            )
            .await
            .is_err(),
        "a resolved request 409s"
    );
}

/// 4. Prompt queueing: a send during a live turn queues, sets `pending_prompt`,
///    and dispatches as a second turn once the first ends.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_queues_during_a_live_turn() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-queue", None, None).await;

    let first = ts
        .client
        .post(
            "/api/sessions/acp-queue/prompt",
            json!({ "text": "wait:700|say:first" }),
        )
        .await
        .unwrap();
    assert_eq!(first["queued"], false);
    assert_eq!(first["turn"], 0);

    // Give the first turn a moment to be in flight, then send again.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let second = ts
        .client
        .post(
            "/api/sessions/acp-queue/prompt",
            json!({ "text": "say:second" }),
        )
        .await
        .unwrap();
    assert_eq!(second["queued"], true, "a send during a turn queues");
    assert_eq!(second["turn"], 0, "queued against the live turn");

    let session = session_mod::get(&ts.state.db, "acp-queue")
        .await
        .unwrap()
        .unwrap();
    assert!(
        session
            .pending_prompt
            .as_deref()
            .unwrap_or("")
            .contains("say:second"),
        "the queue is persisted"
    );

    // The queued prompt dispatches as turn 1 once turn 0 ends.
    let chat = poll_chat(&ts, "acp-queue", Duration::from_secs(10), |blocks| {
        count_kind(blocks, "turn_end") >= 2
    })
    .await;
    let blocks = chat["blocks"].as_array().unwrap();
    assert!(
        blocks.iter().any(|b| b["kind"] == "user_message"
            && b["turn"] == 1
            && b["payload"]["text"] == "say:second"),
        "the queued text became turn 1's user_message"
    );
    assert!(
        blocks
            .iter()
            .any(|b| b["kind"] == "agent_message" && b["payload"]["text"] == "second"),
        "the queued turn ran"
    );
}

/// 5. Crash recovery: stop the loom-side task mid-turn, re-attach, and the
///    replayed frames re-ingest with no duplicate blocks and an advanced cursor.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_recovery_replays_without_duplicates() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-crash", None, None).await;

    ts.client
        .post(
            "/api/sessions/acp-crash/prompt",
            json!({ "text": "say:recovered|wait:1500" }),
        )
        .await
        .unwrap();

    // Let the message chunks stream (buffered, not flushed) while the turn waits.
    tokio::time::sleep(Duration::from_millis(350)).await;
    let before = session_mod::get(&ts.state.db, "acp-crash")
        .await
        .unwrap()
        .unwrap();

    // "Crash" the loom-side task; the relay + agent survive.
    assert!(ts.state.acp.stop("acp-crash"), "a task was running");
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Re-attach: replay from the persisted cursor.
    acp::attach(&ts.state, "acp-crash")
        .await
        .expect("re-attach succeeds");

    let chat = poll_chat(&ts, "acp-crash", Duration::from_secs(10), |blocks| {
        blocks.iter().any(|b| b["kind"] == "turn_end")
    })
    .await;
    let blocks = chat["blocks"].as_array().unwrap();

    // No duplicates: the UNIQUE(session,turn,seq) key held, and recovery did not
    // re-journal committed blocks at fresh seqs.
    assert_eq!(count_kind(blocks, "user_message"), 1, "one user_message");
    assert_eq!(
        count_kind(blocks, "agent_message"),
        1,
        "one agent_message (no dup)"
    );
    assert_eq!(count_kind(blocks, "turn_end"), 1, "one turn_end (no dup)");
    assert!(
        blocks
            .iter()
            .any(|b| b["kind"] == "agent_message" && b["payload"]["text"] == "recovered"),
        "the buffered message rebuilt intact"
    );

    // The ack persists just *after* the block it completes commits (block-boundary
    // acking: never ack a frame until its journal write lands), so the cursor
    // advance trails the visible `turn_end` block — poll for it to settle.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let after = loop {
        let s = session_mod::get(&ts.state.db, "acp-crash")
            .await
            .unwrap()
            .unwrap();
        if s.acp_ack_seq > before.acp_ack_seq || tokio::time::Instant::now() >= deadline {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        after.acp_ack_seq > before.acp_ack_seq,
        "the ack cursor advanced across recovery ({} -> {})",
        before.acp_ack_seq,
        after.acp_ack_seq
    );
}

/// 6. Interrupt: cancelling a waiting turn ends it with stop reason `cancelled`.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupt_cancels_the_turn() {
    let ts = TestServer::start().await;
    start_new(&ts, "acp-int", None, None).await;
    let mut rx = ts.state.acp.get("acp-int").unwrap().subscribe();

    ts.client
        .post(
            "/api/sessions/acp-int/prompt",
            json!({ "text": "wait:3000|say:unreached" }),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;

    let res = ts
        .client
        .post("/api/sessions/acp-int/interrupt", json!({}))
        .await
        .unwrap();
    assert_eq!(res["interrupted"], true);

    let events = drain_events(&mut rx, Duration::from_secs(10), |e| {
        e.event == "turn" && e.data["state"] == "ended"
    })
    .await;
    assert!(
        events.iter().any(|e| e.event == "turn"
            && e.data["state"] == "ended"
            && e.data["stop_reason"] == "cancelled"),
        "the interrupted turn ended cancelled"
    );

    let chat = ts.client.get("/api/sessions/acp-int/chat").await.unwrap();
    let blocks = chat["blocks"].as_array().unwrap();
    let turn_end = blocks
        .iter()
        .find(|b| b["kind"] == "turn_end")
        .expect("a turn_end block");
    assert_eq!(turn_end["payload"]["stop_reason"], "cancelled");
}

/// The `/chat` routes reject a terminal-backend session with 409.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_routes_reject_terminal_sessions() {
    let ts = TestServer::start().await;
    let ws = ts
        .client
        .post(
            "/api/sessions",
            json!({ "goal": "terminal", "cwd": ts.cwd(), "agent": "shell" }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();

    assert!(
        ts.client
            .get(&format!("/api/sessions/{id}/chat"))
            .await
            .is_err(),
        "a terminal session has no chat journal"
    );
    assert!(
        ts.client
            .post(
                &format!("/api/sessions/{id}/prompt"),
                json!({ "text": "hi" })
            )
            .await
            .is_err(),
        "a terminal session has no /prompt"
    );

    ts.client
        .delete(&format!("/api/sessions/{id}"))
        .await
        .unwrap();
}
