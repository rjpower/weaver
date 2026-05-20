//! Background task: mirrors each workspace's tmux screen, detects when a
//! session has ended, and provides screen-stillness idle detection for agents
//! that do not report status via hooks.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde_json::json;

use crate::web::AppState;
use crate::{events, tmux, workspace};

const TICK: Duration = Duration::from_millis(1500);
/// Ticks of an unchanged screen before a non-hook agent is considered idle.
const IDLE_TICKS: u32 = 10;

pub async fn run(state: AppState) {
    let mut screen_hash: HashMap<String, u64> = HashMap::new();
    let mut still_ticks: HashMap<String, u32> = HashMap::new();
    tracing::info!(tick_ms = TICK.as_millis() as u64, "monitor loop started");

    loop {
        tokio::time::sleep(TICK).await;
        let workspaces = match workspace::list(&state.db).await {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("monitor: listing workspaces failed: {e}");
                continue;
            }
        };
        tracing::debug!(workspaces = workspaces.len(), "monitor tick");
        let mut alive: HashSet<String> = HashSet::new();

        for ws in &workspaces {
            alive.insert(ws.id.clone());
            if workspace::is_terminal(&ws.status) {
                continue;
            }
            if !tmux::has_session(&ws.tmux_session).await {
                // A missing session is recoverable (e.g. the tmux daemon died
                // on reboot): mark the workspace `orphaned` so it can later be
                // adopted. Only record this once — re-recording every tick
                // would spam the event log.
                if ws.status != "orphaned" {
                    tracing::info!(id = %ws.id, session = %ws.tmux_session, "tmux session ended; marking orphaned");
                    let _ = workspace::set_status(&state.db, &ws.id, "orphaned").await;
                    let _ = workspace::set_pending_prompt(&state.db, &ws.id, "").await;
                    let _ = events::record(
                        &state.db,
                        &state.bus,
                        &ws.id,
                        "status",
                        json!({ "status": "orphaned", "reason": "tmux session ended" }),
                    )
                    .await;
                }
                continue;
            }

            let screen = tmux::capture(&ws.tmux_session, 0).await.unwrap_or_default();
            let h = hash(&screen);
            if screen_hash.get(&ws.id) != Some(&h) {
                screen_hash.insert(ws.id.clone(), h);
                still_ticks.insert(ws.id.clone(), 0);
                let _ = workspace::touch(&state.db, &ws.id).await;
                events::emit(&state.bus, &ws.id, "screen", json!({ "content": screen }));
            } else {
                let ticks = still_ticks.entry(ws.id.clone()).or_insert(0);
                *ticks += 1;
                // Agents without hooks (anything but claude) get stillness-based
                // idle detection so the UI still reflects reality.
                if ws.agent_kind != "claude" && ws.status == "working" && *ticks >= IDLE_TICKS {
                    tracing::info!(id = %ws.id, still_ticks = *ticks, "screen idle; marking idle");
                    let _ = workspace::set_status(&state.db, &ws.id, "idle").await;
                    let _ = events::record(
                        &state.db,
                        &state.bus,
                        &ws.id,
                        "status",
                        json!({ "status": "idle", "source": "monitor" }),
                    )
                    .await;
                }
            }
        }

        screen_hash.retain(|k, _| alive.contains(k));
        still_ticks.retain(|k, _| alive.contains(k));
    }
}

fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}
