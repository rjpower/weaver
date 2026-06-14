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
use axum::http::header::{HOST, ORIGIN};
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
        // This rejection used to be silent — invisible behind a reverse proxy,
        // where it surfaces only as the browser's endless "reconnecting". Log it
        // with the Origin/Host so a proxy misconfig is one `docker logs` away.
        let origin = header_str(&headers, ORIGIN);
        let host = header_str(&headers, HOST);
        tracing::warn!(
            session = %key,
            origin = %origin,
            host = %host,
            bound = %st.addr,
            "terminal websocket rejected: Origin is neither loopback on the bound \
             port nor the request Host"
        );
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
    tracing::info!(session = %key, target = %target, "terminal websocket attached");
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| async move {
            if let Err(e) = bridge(socket, target.clone()).await {
                tracing::debug!(target = %target, error = %e, "terminal bridge ended with error");
            } else {
                tracing::debug!(target = %target, "terminal bridge detached cleanly");
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
/// A *present* Origin is accepted when either:
///
/// 1. it is a loopback host on our actual bound port — direct localhost access
///    (`loom open`, an SSH tunnel), the original single-user posture; or
/// 2. its host+port equals the request's own `Host` header — i.e. the browser
///    is talking to the same public origin the front-door proxy serves us under
///    (`weaver.example.com`). Behind Caddy the loopback rule can never match
///    (the browser's Origin is the public HTTPS host), so without this the
///    in-browser terminal is wedged on an endless "reconnecting".
///
/// Trusting `Host` reopens the DNS-rebinding vector the loopback pin closed: a
/// rebinding attacker can force `Origin == Host`. We accept that here because
/// this build is deployed behind a reverse proxy that publishes no host port —
/// loom is reachable only through the front door, never directly — so the proxy
/// is the sole arbiter of `Host`. Do NOT rely on this check alone when loom is
/// bound to a publicly reachable port.
fn origin_ok(headers: &HeaderMap, bound_addr: &str) -> bool {
    let origin = match headers.get(ORIGIN) {
        // Non-browser clients (the CLI, tests, tokio-tungstenite) omit Origin.
        None => return true,
        Some(v) => match v.to_str() {
            Ok(o) => o,
            Err(_) => return false,
        },
    };
    if origin_is_loopback(origin, bound_addr) {
        return true;
    }
    match headers.get(HOST).and_then(|v| v.to_str().ok()) {
        Some(host) => origin_matches_host(origin, host),
        None => false,
    }
}

/// Read a header as a `&str`, or `"<none>"` when absent/non-ASCII. For logging.
fn header_str<'a>(headers: &'a HeaderMap, name: axum::http::HeaderName) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>")
}

/// Split an authority (`host`, `host:port`, `[::1]`, or `[::1]:port`) into its
/// host and optional port. Shared by the loopback and proxied-Host checks so
/// both parse IPv6 literals identically.
fn split_authority(authority: &str) -> (&str, Option<&str>) {
    if let Some(after) = authority.strip_prefix('[') {
        // IPv6 literal: [::1] or [::1]:port
        match after.split_once(']') {
            Some((h, tail)) => (h, tail.strip_prefix(':')),
            None => (authority, None),
        }
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        (h, Some(p))
    } else {
        (authority, None)
    }
}

/// True iff `origin` (`http(s)://host[:port]`) names the same host+port as the
/// request's `Host` header. The browser sends the page's own origin on a WS
/// handshake, so behind a proxy serving `https://h/` the Origin is `https://h`
/// and the Host is `h` — they agree once default ports (443/80) are filled in.
fn origin_matches_host(origin: &str, host: &str) -> bool {
    let (scheme, rest) = if let Some(r) = origin.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = origin.strip_prefix("http://") {
        ("http", r)
    } else {
        return false;
    };
    let origin_authority = rest.split('/').next().unwrap_or(rest);
    let (o_host, o_port) = split_authority(origin_authority);
    let (h_host, h_port) = split_authority(host);
    if o_host.is_empty() || o_host != h_host {
        return false;
    }
    let default = if scheme == "https" { "443" } else { "80" };
    o_port.unwrap_or(default) == h_port.unwrap_or(default)
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
    let (host, port) = split_authority(authority);
    let port = port.unwrap_or(if scheme == "https" { "443" } else { "80" });
    if port != want_port {
        return false;
    }
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

#[cfg(test)]
mod tests {
    use super::{origin_is_loopback, origin_matches_host};

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

    #[test]
    fn accepts_origin_matching_proxied_host() {
        // Behind Caddy: browser at https://weaver.rjp.io, proxy forwards
        // Host: weaver.rjp.io. Default https port (443) fills in on both sides.
        assert!(origin_matches_host(
            "https://weaver.rjp.io",
            "weaver.rjp.io"
        ));
        assert!(origin_matches_host("https://loom.rjp.io", "loom.rjp.io"));
        // Explicit matching ports also agree.
        assert!(origin_matches_host(
            "https://h.example:8443",
            "h.example:8443"
        ));
        // Plain http with the default port elided on the Origin side.
        assert!(origin_matches_host("http://box.local", "box.local:80"));
    }

    #[test]
    fn rejects_origin_not_matching_host() {
        // A different host (the cross-site attacker case) never matches.
        assert!(!origin_matches_host(
            "https://attacker.example",
            "weaver.rjp.io"
        ));
        // Same host, mismatched explicit port.
        assert!(!origin_matches_host(
            "https://weaver.rjp.io:1234",
            "weaver.rjp.io"
        ));
        // Non-http scheme and garbage.
        assert!(!origin_matches_host("ftp://weaver.rjp.io", "weaver.rjp.io"));
        assert!(!origin_matches_host("null", "weaver.rjp.io"));
    }
}
