//! WebSocket ⇄ terminal bridge: the browser's xterm.js talks to the session's
//! [`tapestry`] supervisor, which streams raw PTY bytes — so the browser gets a
//! real terminal (colour, cursor, mouse, and xterm's own scrollback, selection,
//! and search) instead of a polled read-only mirror.
//!
//! This is a parallel, purely-interactive channel; the hooks → events → SSE data
//! plane and the monitor heartbeat are untouched.
//!
//! ## Wire protocol
//!
//! Binary frames both directions. server → client is raw PTY output bytes. The
//! client → server hot path is opcode-prefixed (the ttyd/terminado/VS Code
//! shape):
//!
//! * `0x00 <bytes…>` — keystrokes, forwarded verbatim to the PTY writer.
//! * `0x01 <cols:u16_be> <rows:u16_be>` — resize (exactly 5 bytes).
//!
//! Text frames are tolerated as raw keystrokes (no opcode); the frontend always
//! sends binary. Empty frames, malformed resizes, and unknown opcodes are
//! dropped.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};

use crate::backend;
use crate::web::{require_session, AppState};

/// Cap on a single inbound WebSocket frame. Keystrokes and resizes are tiny and
/// even a large paste is bounded; this stops a hostile client forcing a huge
/// allocation before we inspect the opcode.
const MAX_FRAME: usize = 64 * 1024;

const OP_INPUT: u8 = 0x00;
const OP_RESIZE: u8 = 0x01;

/// `GET /api/sessions/{id}/terminal` — upgrade to a WebSocket bridged to the
/// session's terminal supervisor.
pub async fn terminal_ws(
    ws: WebSocketUpgrade,
    State(st): State<AppState>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> Response {
    // CSWSH defence: CORS does NOT apply to WebSockets, so a localhost bind is
    // not protection — validate the Origin before upgrading. See `origin_ok`.
    if !origin_ok(&headers, &st.addr) {
        return (StatusCode::FORBIDDEN, "cross-origin websocket rejected").into_response();
    }
    let session = match require_session(&st.db, &key).await {
        Ok((s, _)) => s,
        Err(_) => return (StatusCode::NOT_FOUND, "no such session").into_response(),
    };
    // No live supervisor means nothing to attach to — the caller should adopt first.
    if !backend::has_session(&session.term_session).await {
        return (
            StatusCode::CONFLICT,
            "session has no running terminal — adopt it first",
        )
            .into_response();
    }
    let target = session.term_session.clone();
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| async move {
            if let Err(e) = bridge(socket, target).await {
                tracing::debug!(error = %e, "terminal bridge ended with error");
            }
        })
}

/// Bridge a browser xterm to a session's [`tapestry`] supervisor: the supervisor
/// streams raw PTY bytes, so this is a straight byte pump. Dropping the input
/// half closes the socket write half, which the supervisor sees as a clean
/// detach — the child keeps running, and a refresh reconnects and repaints.
async fn bridge(socket: WebSocket, target: String) -> anyhow::Result<()> {
    let client = tapestry::Client::connect(&target).await?;
    // Start at 80×24; the client sends a real size in its first fit.
    let attach = client.attach(80, 24).await?;
    let (mut input, mut output) = attach.split();
    let (mut sink, mut stream) = socket.split();

    // PTY output → ws.
    let mut out_task = tokio::spawn(async move {
        while let Some(chunk) = output.recv().await {
            if sink.send(Message::Binary(chunk.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    });

    // ws → PTY input / resize.
    let mut in_task = tokio::spawn(async move {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Binary(payload)) => {
                    if payload.is_empty() {
                        continue;
                    }
                    match payload[0] {
                        OP_INPUT => {
                            if input.send_input(&payload[1..]).await.is_err() {
                                break;
                            }
                        }
                        OP_RESIZE if payload.len() == 5 => {
                            let cols = u16::from_be_bytes([payload[1], payload[2]]).clamp(1, 1000);
                            let rows = u16::from_be_bytes([payload[3], payload[4]]).clamp(1, 1000);
                            if input.resize(cols, rows).await.is_err() {
                                break;
                            }
                        }
                        _ => continue, // malformed resize / unknown opcode
                    }
                }
                Ok(Message::Text(t)) => {
                    if input.send_input(t.as_str().as_bytes()).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue, // Ping/Pong handled by axum
            }
        }
    });

    // Whichever pump finishes first, drop the other — dropping `input` closes the
    // socket write half, which the supervisor sees as a clean detach (the child
    // keeps running, a refresh reconnects and repaints).
    tokio::select! {
        _ = &mut out_task => in_task.abort(),
        _ = &mut in_task => out_task.abort(),
    }
    Ok(())
}

/// Whether to allow the WebSocket upgrade.
///
/// A *missing* Origin (non-browser clients: the CLI, tests, `tokio-tungstenite`)
/// is allowed — browsers always send Origin on a WS handshake, so this does not
/// weaken the browser-CSWSH defence (it only waives same-host non-browser
/// processes, acceptable under the single-user-localhost assumption).
///
/// A *present* Origin must be a loopback host on our actual bound port. We
/// deliberately do NOT compare against the request `Host` header: Host is
/// client-supplied and a DNS-rebinding attacker can make `Origin == Host` even
/// on the default `127.0.0.1` bind. Pinning to the loopback set + the real bound
/// port closes that vector.
fn origin_ok(headers: &HeaderMap, bound_addr: &str) -> bool {
    match headers.get(axum::http::header::ORIGIN) {
        None => true,
        Some(v) => match v.to_str() {
            Ok(origin) => origin_is_loopback(origin, bound_addr),
            Err(_) => false,
        },
    }
}

/// True iff `origin` is `http(s)://<loopback>[:<port>]` whose port equals the
/// port of `bound_addr`.
fn origin_is_loopback(origin: &str, bound_addr: &str) -> bool {
    let Some((_, want_port)) = bound_addr.rsplit_once(':') else {
        return false;
    };
    let (scheme, rest) = if let Some(r) = origin.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = origin.strip_prefix("https://") {
        ("https", r)
    } else {
        return false;
    };
    // Origin carries no path, but be defensive about a trailing slash.
    let authority = rest.split('/').next().unwrap_or(rest);
    let (host, port) = if let Some(after) = authority.strip_prefix('[') {
        // IPv6 literal: [::1] or [::1]:port
        match after.split_once(']') {
            Some((h, tail)) => (h, tail.strip_prefix(':')),
            None => return false,
        }
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        (h, Some(p))
    } else {
        (authority, None)
    };
    let port = port.unwrap_or(if scheme == "https" { "443" } else { "80" });
    if port != want_port {
        return false;
    }
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

#[cfg(test)]
mod tests {
    use super::origin_is_loopback;

    #[test]
    fn accepts_loopback_origins_on_the_bound_port() {
        assert!(origin_is_loopback(
            "http://127.0.0.1:7878",
            "127.0.0.1:7878"
        ));
        assert!(origin_is_loopback(
            "http://localhost:7878",
            "127.0.0.1:7878"
        ));
        assert!(origin_is_loopback("http://[::1]:7878", "127.0.0.1:7878"));
        // Bound to all interfaces but the browser reached us via loopback.
        assert!(origin_is_loopback("http://localhost:9999", "0.0.0.0:9999"));
    }

    #[test]
    fn rejects_wrong_port() {
        assert!(!origin_is_loopback(
            "http://127.0.0.1:1234",
            "127.0.0.1:7878"
        ));
        // No port on the origin defaults to 80/443, which won't match.
        assert!(!origin_is_loopback("http://127.0.0.1", "127.0.0.1:7878"));
    }

    #[test]
    fn rejects_non_loopback_host() {
        // The DNS-rebinding vector: attacker.com rebound to 127.0.0.1 sends
        // Origin: http://attacker.com:7878 — not in the loopback set → rejected.
        assert!(!origin_is_loopback(
            "http://attacker.com:7878",
            "127.0.0.1:7878"
        ));
        assert!(!origin_is_loopback(
            "http://192.168.1.5:7878",
            "0.0.0.0:7878"
        ));
    }

    #[test]
    fn rejects_non_http_scheme_and_garbage() {
        assert!(!origin_is_loopback(
            "ftp://127.0.0.1:7878",
            "127.0.0.1:7878"
        ));
        assert!(!origin_is_loopback("null", "127.0.0.1:7878"));
        assert!(!origin_is_loopback("", "127.0.0.1:7878"));
    }
}
