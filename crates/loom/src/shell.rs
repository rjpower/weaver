//! The operator **scratch shell** — a single, always-available login shell
//! inside the container, reachable from the web UI (the "Shell" nav item) over
//! the same WebSocket terminal bridge agent sessions use.
//!
//! Unlike an agent session it is not tied to any branch or worktree: it is one
//! persistent [`tapestry`] supervisor (named [`SHELL_SESSION`]) running a plain
//! login shell in the container user's `$HOME`. Its purpose is one-time operator
//! setup that would otherwise need `docker exec -it weaver …` — most concretely
//! `gcloud auth login`, whose credentials land in the `CLOUDSDK_CONFIG` volume
//! and so survive recreates (see the Dockerfile / docker-compose.yml).
//!
//! Because tapestry supervisors are detached and outlive `loom server run`, the
//! shell — and any login half-finished in it — persists across a loom restart,
//! exactly like an agent terminal. It is spawned lazily on first attach
//! ([`ensure`]) and can be reset with [`restart`].

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::agent::{self, LaunchMode};
use crate::backend;
use crate::{agent_env, AppState};

/// The fixed supervisor name for the operator scratch shell. Distinct from any
/// agent session's `term_session` (those are random ids), so it never collides.
pub const SHELL_SESSION: &str = "loom-scratch-shell";

/// Where the scratch shell starts: the container user's `$HOME` (the persisted
/// `/home/app` volume). Falls back to `/` if `$HOME` is somehow unset. The cwd
/// barely matters for its main job — `gcloud`/`gh` write to global config paths
/// — but `$HOME` is the least surprising place to land.
fn shell_cwd() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Ensure the scratch-shell supervisor is up, spawning it if not. Idempotent: a
/// live shell is left untouched (so reconnecting/refreshing the UI reattaches to
/// the same session rather than starting a new one).
///
/// The launch script exports the same operator surface an agent session gets —
/// `WEAVER_API` + `LOOM_TOKEN` so the in-container `weaver`/`loom` CLIs work, and
/// the operator-managed [`agent_env`] vars — then `exec`s the login shell. It is
/// deliberately *not* given a `WEAVER_BRANCH`: there's no branch behind it.
pub async fn ensure(st: &AppState) -> Result<()> {
    if backend::has_session(SHELL_SESSION).await {
        return Ok(());
    }

    let api_url = format!("http://{}", st.addr);
    let local_token = agent::read_local_token();
    let extra = agent_env::pairs(&st.db).await.unwrap_or_default();

    let mut env: Vec<(&str, &str)> = vec![("WEAVER_API", api_url.as_str())];
    if let Some(token) = local_token.as_deref() {
        env.push(("LOOM_TOKEN", token));
    }
    for (k, v) in &extra {
        env.push((k.as_str(), v.as_str()));
    }

    let loom_exe = std::env::current_exe().ok();
    let weaver_dir = loom_exe.as_deref().and_then(Path::parent);
    // `agent_kind = "shell"` ⇒ no inner command, just the env exports followed by
    // `exec "${SHELL:-/bin/sh}"`. Adopt/empty args are irrelevant for a shell.
    let script = agent::launch_script("shell", None, &env, weaver_dir, LaunchMode::Adopt, "");

    let cwd = shell_cwd();
    tracing::info!(session = SHELL_SESSION, cwd = %cwd.display(), "spawning operator scratch shell");
    backend::new_session(SHELL_SESSION, &cwd, &script).await
}

/// Reset the scratch shell: kill the current supervisor (best-effort) and bring
/// a fresh one up. Used by `POST /api/shell/restart` — e.g. to pick up newly
/// edited operator env vars, or to clear a wedged session.
pub async fn restart(st: &AppState) -> Result<()> {
    backend::kill_session(SHELL_SESSION).await.ok();
    // The supervisor removes its socket as it exits; wait briefly for liveness to
    // drop so `ensure` actually respawns rather than re-adopting the dying one.
    for _ in 0..20 {
        if !backend::has_session(SHELL_SESSION).await {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    ensure(st).await
}
