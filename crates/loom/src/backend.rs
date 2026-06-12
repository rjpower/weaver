//! The terminal-management seam.
//!
//! Loom's programmatic terminal operations — create a session, check liveness,
//! capture the screen, type into it, kill it — go through here rather than
//! calling `tmux` directly, so the underlying supervisor is swappable. Two
//! backends exist:
//!
//! * **tmux** (default): the original `tmux new-session / capture-pane /
//!   send-keys / kill-session` shell-outs in [`crate::tmux`].
//! * **tapestry**: a per-session detached PTY supervisor ([`tapestry`]) that
//!   streams raw PTY bytes, so an attached xterm owns its own scrollback,
//!   selection, and search instead of rendering a re-rendered tmux screen.
//!
//! The backend is chosen once from `WEAVER_TERMINAL_BACKEND` (`tmux` |
//! `tapestry`), defaulting to `tmux` so behaviour is unchanged until a deployment
//! opts in. The interactive attach surface (the xterm WebSocket bridge in
//! [`crate::terminal`] and the `loom session attach` CLI) branches on
//! [`selected`] directly, because attaching to a tmux client and to a tapestry
//! socket are structurally different.

use anyhow::Result;

/// Which terminal backend manages sessions this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Tmux,
    Tapestry,
}

/// The backend selected by `WEAVER_TERMINAL_BACKEND` (default [`Backend::Tmux`]).
/// An unrecognized value falls back to tmux with a warning rather than failing
/// startup.
pub fn selected() -> Backend {
    match std::env::var("WEAVER_TERMINAL_BACKEND").ok().as_deref() {
        Some("tapestry") => Backend::Tapestry,
        Some("tmux") | None | Some("") => Backend::Tmux,
        Some(other) => {
            tracing::warn!(
                value = other,
                "unknown WEAVER_TERMINAL_BACKEND; falling back to tmux"
            );
            Backend::Tmux
        }
    }
}

/// Whether a session with this name has a live supervisor (`tmux has-session`).
pub async fn has_session(name: &str) -> bool {
    match selected() {
        Backend::Tmux => crate::tmux::has_session(name).await,
        Backend::Tapestry => tapestry::Client::is_alive(name).await,
    }
}

/// Create a detached session running `script` via `sh -c` in `cwd`.
pub async fn new_session(name: &str, cwd: &std::path::Path, script: &str) -> Result<()> {
    match selected() {
        Backend::Tmux => crate::tmux::new_session(name, cwd, script).await,
        Backend::Tapestry => {
            let bin = tapestry_bin();
            tapestry::spawn_detached(&tapestry::LaunchOptions {
                name,
                cwd,
                script,
                // The launch script already bakes the agent env in as `export`
                // statements (see agent::launch_script), matching the tmux path.
                env: &[],
                cols: 80,
                rows: 24,
                supervisor_bin: bin.as_deref(),
            })
            .await
        }
    }
}

/// Render the session's screen to text; `history` extra scrollback lines.
pub async fn capture(name: &str, history: usize) -> Result<String> {
    match selected() {
        Backend::Tmux => crate::tmux::capture(name, history).await,
        Backend::Tapestry => {
            let mut c = tapestry::Client::connect(name).await?;
            c.capture(history as u32).await
        }
    }
}

/// Type `text` into the session verbatim (the `send-keys -l` analogue). No
/// trailing newline; pair with [`send_enter`] to submit.
pub async fn send_literal(name: &str, text: &str) -> Result<()> {
    match selected() {
        Backend::Tmux => crate::tmux::send_literal(name, text).await,
        Backend::Tapestry => {
            let mut c = tapestry::Client::connect(name).await?;
            c.send(text.as_bytes()).await
        }
    }
}

/// Send a single named key (e.g. `Enter`, `Escape`) to the session.
pub async fn send_key(name: &str, key: &str) -> Result<()> {
    match selected() {
        Backend::Tmux => crate::tmux::send_key(name, key).await,
        Backend::Tapestry => {
            let mut c = tapestry::Client::connect(name).await?;
            c.send(key_bytes(key)).await
        }
    }
}

/// Submit the current input — a bare `Enter`.
pub async fn send_enter(name: &str) -> Result<()> {
    send_key(name, "Enter").await
}

/// Kill the session (`tmux kill-session`). Best-effort: a missing session is fine.
pub async fn kill_session(name: &str) -> Result<()> {
    match selected() {
        Backend::Tmux => crate::tmux::kill_session(name).await,
        Backend::Tapestry => {
            // A missing supervisor means already gone — not an error.
            if let Ok(mut c) = tapestry::Client::connect(name).await {
                let _ = c.kill().await;
            }
            Ok(())
        }
    }
}

/// Every session with a live supervisor (`tmux list-sessions`).
pub async fn list_sessions() -> Result<Vec<String>> {
    match selected() {
        Backend::Tmux => crate::tmux::list_sessions().await,
        Backend::Tapestry => Ok(tapestry::list_sessions().await),
    }
}

/// The `tapestry` supervisor binary, resolved as a sibling of the running `loom`
/// executable (they ship together), so the detached supervisor is the real
/// `tapestry` binary rather than `loom` (whose `current_exe` lacks a `supervise`
/// subcommand). `None` falls back to `tapestry` on `PATH`. An explicit
/// `WEAVER_TAPESTRY_BIN` overrides both — used by the integration tests, whose
/// `loom` and `tapestry` are built into the same target dir.
fn tapestry_bin() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("WEAVER_TAPESTRY_BIN") {
        return Some(std::path::PathBuf::from(p));
    }
    std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(|d| d.join("tapestry"))
        .filter(|p| p.exists())
}

/// Translate the small set of tmux key names loom uses into the raw bytes a PTY
/// expects. Unknown names fall through as their literal text.
fn key_bytes(key: &str) -> &[u8] {
    match key {
        "Enter" => b"\r",
        "Escape" => b"\x1b",
        "Tab" => b"\t",
        "Space" => b" ",
        "BSpace" => b"\x7f",
        other => other.as_bytes(),
    }
}
