//! The Overlooker engine: the dispatcher's event→trigger matching (T4), a stock
//! `builtin:status` round landing a mark and its audit row (T5/T11), and the
//! guardrails (cooldown / no-overlap → `skipped`).
//!
//! These cases drive the engine **directly** on the test server's isolated db
//! rather than through the spawned background loop: the `overlooker.enabled`
//! master switch is left at its default (off), so the daemon's own engine idles
//! and never races these deterministic calls. Each test builds its own
//! `AppState` over the same isolated db (the harness exports `WEAVER_HOME`) and
//! calls the public engine seams — `dispatch`, `fire_now`, `new_in_flight` /
//! `fire` — so a round runs without waiting on the timer.

use std::collections::HashSet;

use chrono::Utc;
use serde_json::json;
use serial_test::serial;

use loom::events::EventBus;
use loom::web::AppState;
use loom::{db, events, monitor, overlooker, session as session_mod};
use weaver_core::overlooker as ov;

use crate::fixtures::TestServer;

/// An `AppState` over the test server's isolated db — a second connection to the
/// same sqlite file (WAL, so concurrent readers/writers are fine). Lets a test
/// call the in-process engine seams the spawned loop would otherwise own.
async fn engine_state(ts: &TestServer) -> AppState {
    let pool = db::connect(&db::default_db_path()).await.unwrap();
    AppState {
        db: pool,
        bus: EventBus::new(),
        addr: ts.addr.to_string(),
    }
}

/// Create a `shell` session and return `(session_id, branch_id, repo_root)`.
async fn make_session(ts: &TestServer, goal: &str) -> (String, String, String) {
    let ws = ts
        .client
        .post(
            "/api/sessions",
            json!({ "goal": goal, "cwd": ts.cwd(), "agent": "shell" }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();
    let branch_id = ws["branch"]["id"].as_str().unwrap().to_string();
    let repo_root = ws["branch"]["repo_root"].as_str().unwrap().to_string();
    (id, branch_id, repo_root)
}

/// Register an enabled overlooker and return it.
async fn enabled_overlooker(state: &AppState, new: ov::NewOverlooker) -> ov::Overlooker {
    let o = ov::create(&state.db, &new).await.unwrap();
    ov::set_enabled(&state.db, &o.id, true).await.unwrap();
    ov::get(&state.db, &o.id).await.unwrap().unwrap()
}

/// T4: a reactive event whose trigger matches fires the right overlooker exactly
/// once and lands a run row; a repo filter excludes another repo's event; and
/// re-firing the same event is idempotent (the mark converges, no contradiction).
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispatcher_matches_trigger_with_repo_filter_and_is_idempotent() {
    let ts = TestServer::start().await;
    let state = engine_state(&ts).await;
    let (session_id, branch_id, repo_root) = make_session(&ts, "watch me").await;

    // The agent declares `blocked` about itself (the reactive signal).
    ts.client
        .patch(
            &format!("/api/sessions/{session_id}"),
            json!({ "attention": "blocked" }),
        )
        .await
        .unwrap();

    // An overlooker reacting to `attention` events in *this* repo, marking the
    // scoped (non-ok) fleet.
    let o = enabled_overlooker(
        &state,
        ov::NewOverlooker {
            name: "blocked-watch".to_string(),
            trigger_spec: json!({ "event": "attention", "repo": repo_root }).to_string(),
            scope: json!({ "attention": "!ok" }).to_string(),
            program: "builtin:status".to_string(),
            capabilities: vec!["observe".to_string(), "mark".to_string()],
            ..Default::default()
        },
    )
    .await;

    let in_flight = overlooker::new_in_flight();

    // A matching reactive event (attention=blocked on a branch in this repo).
    let ev = events::Event {
        id: 0,
        branch_id: branch_id.clone(),
        kind: "attention".to_string(),
        data: json!({ "level": "blocked" }),
        created_at: db::now_iso(),
    };
    overlooker::dispatch(&state, &in_flight, &ev).await;

    // Exactly one run, and it marked the session.
    let runs = ov::recent_runs(&state.db, &o.id, 10).await.unwrap();
    assert_eq!(runs.len(), 1, "one matching event fires exactly one round");
    assert_eq!(runs[0].outcome, "ok");
    let view = ts
        .client
        .get(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
    assert_eq!(
        view["branch"]["triage_level"], "blocked",
        "the rule mirrors the agent's attention onto the mark"
    );
    assert_eq!(view["branch"]["triage_by"], "blocked-watch");

    // Re-firing the identical event is idempotent: it converges on the same mark
    // (a fresh run, but no contradictory state). Level-triggered: the round
    // re-surveys rather than "handling" the event again.
    overlooker::dispatch(&state, &in_flight, &ev).await;
    let view2 = ts
        .client
        .get(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
    assert_eq!(
        view2["branch"]["triage_level"], "blocked",
        "re-firing converges on the same mark, not a contradiction"
    );

    // The repo filter excludes an event from another repo: same kind/level but a
    // branch that lives elsewhere must not fire this overlooker.
    let runs_before = ov::recent_runs(&state.db, &o.id, 100).await.unwrap().len();
    let other_repo_event = events::Event {
        id: 0,
        // A system (branchless) event carries no repo, so the repo-filtered
        // trigger must not match it.
        branch_id: events::SYSTEM_BRANCH.to_string(),
        kind: "attention".to_string(),
        data: json!({ "level": "blocked" }),
        created_at: db::now_iso(),
    };
    overlooker::dispatch(&state, &in_flight, &other_repo_event).await;
    let runs_after = ov::recent_runs(&state.db, &o.id, 100).await.unwrap().len();
    assert_eq!(
        runs_before, runs_after,
        "a repo-filtered trigger ignores an event from outside its repo"
    );

    ts.client
        .delete(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
}

/// T7: a session that crosses the staleness threshold causes the monitor to emit
/// a one-shot `stale` event, an overlooker with an `{"event":"stale"}` trigger
/// fires a round off it, and the emission is edge-detected — driving the
/// staleness check twice produces exactly one `stale` row, not one per pass.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_session_emits_one_event_and_wakes_a_reactive_overlooker() {
    let ts = TestServer::start().await;
    let state = engine_state(&ts).await;
    let (session_id, _branch_id, _repo_root) = make_session(&ts, "go stale").await;

    // The session reports `attention` about itself so the stock round's rule
    // produces a non-ok mark when the overlooker fires (proving it ran).
    ts.client
        .patch(
            &format!("/api/sessions/{session_id}"),
            json!({ "attention": "attention" }),
        )
        .await
        .unwrap();

    // A reactive overlooker matching `stale` events fleet-wide.
    let o = enabled_overlooker(
        &state,
        ov::NewOverlooker {
            name: "stale-watch".to_string(),
            trigger_spec: json!({ "event": "stale" }).to_string(),
            scope: json!({ "attention": "!ok" }).to_string(),
            program: "builtin:status".to_string(),
            capabilities: vec!["observe".to_string(), "mark".to_string()],
            ..Default::default()
        },
    )
    .await;

    // Drive the monitor's staleness check directly with a zero threshold (stale
    // immediately) — the deterministic, fast stand-in for waiting 30 minutes.
    let session = session_mod::get(&state.db, &session_id)
        .await
        .unwrap()
        .unwrap();
    let watermark = events::max_id(&state.db).await.unwrap();
    let mut seen: HashSet<String> = HashSet::new();
    let now = Utc::now();

    // Two passes: the edge fires on the first, the second is a no-op.
    monitor::detect_stale(&state, &session, 0, now, &mut seen, watermark).await;
    monitor::detect_stale(&state, &session, 0, now, &mut seen, watermark).await;

    // Exactly one `stale` event was emitted across both passes (edge-detected).
    let new_events = events::since(&state.db, watermark).await.unwrap();
    let stale: Vec<_> = new_events.iter().filter(|e| e.kind == "stale").collect();
    assert_eq!(
        stale.len(),
        1,
        "the monitor emits `stale` once per transition, not every tick: {new_events:?}"
    );
    let stale_ev = stale[0];
    assert_eq!(
        stale_ev.data["session"],
        session_id.as_str(),
        "the stale event names the idle session"
    );
    assert!(
        stale_ev.data["idle_secs"].is_number(),
        "the stale event carries an idle_secs"
    );
    assert!(
        !events::is_system(&stale_ev.branch_id),
        "a stale event is branch-scoped so its repo resolves for repo-filtering"
    );

    // The dispatcher consumes the stale tick and fires the reactive round.
    let in_flight = overlooker::new_in_flight();
    overlooker::dispatch(&state, &in_flight, stale_ev).await;
    let runs = ov::recent_runs(&state.db, &o.id, 10).await.unwrap();
    assert_eq!(runs.len(), 1, "the stale event fires exactly one round");
    let view = ts
        .client
        .get(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
    assert_eq!(
        view["branch"]["triage_level"], "attention",
        "the woken round marked the in-scope stale session"
    );
    assert_eq!(view["branch"]["triage_by"], "stale-watch");

    // Once activity resumes (a non-stale pass clears the session from `seen`),
    // the edge re-arms: a later stale crossing emits a fresh event.
    let watermark2 = events::max_id(&state.db).await.unwrap();
    // A high threshold makes this just-touched session read as not-stale, which
    // re-arms the edge…
    monitor::detect_stale(&state, &session, 100_000, now, &mut seen, watermark2).await;
    assert!(
        !seen.contains(&session_id),
        "a not-stale pass clears the session, re-arming the edge"
    );
    // …so crossing the threshold again emits a second `stale` event.
    monitor::detect_stale(&state, &session, 0, now, &mut seen, watermark2).await;
    let after = events::since(&state.db, watermark2).await.unwrap();
    assert_eq!(
        after.iter().filter(|e| e.kind == "stale").count(),
        1,
        "after the edge re-arms, a new crossing emits again"
    );

    ts.client
        .delete(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
}

/// T5 / T11: a `builtin:status` round over a non-ok session lands a triage mark
/// (rule path, no real claude) and records the action in the run; a `dry_run`
/// round makes no mark but records a `would`-action.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn builtin_status_marks_a_session_and_dry_run_is_safe() {
    let ts = TestServer::start().await;
    let state = engine_state(&ts).await;
    let (session_id, _branch_id, _repo_root) = make_session(&ts, "status me").await;

    // The session reports `attention` about itself → it's in a `!ok` scope.
    ts.client
        .patch(
            &format!("/api/sessions/{session_id}"),
            json!({ "attention": "attention" }),
        )
        .await
        .unwrap();

    let o = enabled_overlooker(
        &state,
        ov::NewOverlooker {
            name: "status-check".to_string(),
            trigger_spec: json!({ "cron": "0 * * * *" }).to_string(),
            scope: json!({ "attention": "!ok" }).to_string(),
            program: "builtin:status".to_string(),
            capabilities: vec!["observe".to_string(), "mark".to_string()],
            ..Default::default()
        },
    )
    .await;

    // A manual round (run-now) fires the stock program directly.
    let run_id = overlooker::fire_now(&state, &o.name, false, "manual")
        .await
        .unwrap();

    // The mark landed on the session's branch.
    let view = ts
        .client
        .get(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
    assert_eq!(
        view["branch"]["triage_level"], "attention",
        "the round marks the in-scope session"
    );
    assert_eq!(view["branch"]["triage_by"], "status-check");

    // The run row records the mark action.
    let runs = ov::recent_runs(&state.db, &o.id, 10).await.unwrap();
    let run = runs.iter().find(|r| r.id == run_id).unwrap();
    assert_eq!(run.outcome, "ok");
    let actions: serde_json::Value = serde_json::from_str(&run.actions).unwrap();
    let arr = actions.as_array().unwrap();
    assert!(
        arr.iter()
            .any(|a| a["action"] == "mark" && a["session"] == session_id.as_str()),
        "the run records a mark action: {actions}"
    );

    // Clear the mark, then a dry run must NOT re-apply it — it records a `would`.
    ts.client
        .post(
            &format!("/api/sessions/{session_id}/triage"),
            json!({ "level": "", "by": "manual" }),
        )
        .await
        .unwrap();
    let dry_run_id = overlooker::fire_now(&state, &o.name, true, "manual")
        .await
        .unwrap();
    let view = ts
        .client
        .get(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
    assert_eq!(
        view["branch"]["triage_level"], "",
        "a dry run applies no mark"
    );
    let runs = ov::recent_runs(&state.db, &o.id, 10).await.unwrap();
    let dry = runs.iter().find(|r| r.id == dry_run_id).unwrap();
    let actions: serde_json::Value = serde_json::from_str(&dry.actions).unwrap();
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["would"] == "mark" && a["session"] == session_id.as_str()),
        "a dry run logs a would-mark instead of marking: {actions}"
    );
    assert!(
        dry.summary.contains("dry run"),
        "the dry-run summary says so: {}",
        dry.summary
    );

    ts.client
        .delete(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
}

/// T6: the timer half emits a `cron` system tick for a due scheduled overlooker
/// (visible in the event log) and advances its `next_run_at`; that tick then
/// drives a round through the dispatcher — the producer→consumer chain unattended.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timer_emits_cron_tick_for_a_due_overlooker_and_dispatches_it() {
    let ts = TestServer::start().await;
    let state = engine_state(&ts).await;
    let (session_id, _branch_id, _repo_root) = make_session(&ts, "tick me").await;
    ts.client
        .patch(
            &format!("/api/sessions/{session_id}"),
            json!({ "attention": "attention" }),
        )
        .await
        .unwrap();

    let o = enabled_overlooker(
        &state,
        ov::NewOverlooker {
            // `every` sugar so the next-fire is deterministic duration arithmetic.
            trigger_spec: json!({ "every": "30m" }).to_string(),
            name: "hourly".to_string(),
            scope: json!({ "attention": "!ok" }).to_string(),
            program: "builtin:status".to_string(),
            capabilities: vec!["observe".to_string(), "mark".to_string()],
            ..Default::default()
        },
    )
    .await;

    // Force it due: set next_run_at into the past.
    ov::set_schedule(&state.db, &o.id, None, Some("2000-01-01T00:00:00.000Z"))
        .await
        .unwrap();

    let watermark = events::max_id(&state.db).await.unwrap();
    overlooker::tick_timer(&state).await;

    // The tick is a first-class, logged `cron` system event carrying our id.
    let new_events = events::since(&state.db, watermark).await.unwrap();
    let cron = new_events
        .iter()
        .find(|e| e.kind == "cron" && e.data["overlooker"] == o.id.as_str())
        .expect("the timer emits a cron tick for the due overlooker");
    assert!(
        events::is_system(&cron.branch_id),
        "a cron tick is a fleet-global (system) row"
    );

    // It advanced next_run_at into the future, so it won't re-fire every tick.
    let after = ov::get(&state.db, &o.id).await.unwrap().unwrap();
    assert!(
        after.next_run_at.is_some(),
        "the timer advances next_run_at"
    );
    assert_ne!(
        after.next_run_at.as_deref(),
        Some("2000-01-01T00:00:00.000Z"),
        "next_run_at moved forward off the past due time"
    );

    // The dispatcher consumes that cron tick and runs the scheduled round.
    let in_flight = overlooker::new_in_flight();
    overlooker::dispatch(&state, &in_flight, cron).await;
    let runs = ov::recent_runs(&state.db, &o.id, 10).await.unwrap();
    assert_eq!(runs.len(), 1, "the cron tick fires exactly one round");
    let view = ts
        .client
        .get(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
    assert_eq!(
        view["branch"]["triage_level"], "attention",
        "the scheduled round marked the in-scope session"
    );

    ts.client
        .delete(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
}

/// Guardrails: a cooldown re-fire and an in-flight re-fire are both refused — the
/// cooldown one recorded `skipped`, the in-flight one dropped silently (it never
/// opened a run row, because it never started).
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cooldown_and_overlap_refire_are_refused() {
    let ts = TestServer::start().await;
    let state = engine_state(&ts).await;
    let (session_id, branch_id, repo_root) = make_session(&ts, "cool down").await;
    ts.client
        .patch(
            &format!("/api/sessions/{session_id}"),
            json!({ "attention": "blocked" }),
        )
        .await
        .unwrap();

    // A reactive overlooker with a long cooldown, so a second reactive fire
    // inside the gap is skipped. (A manual run-now bypasses cooldown by design,
    // so the reactive path is what exercises this guardrail.)
    let o = enabled_overlooker(
        &state,
        ov::NewOverlooker {
            name: "cooldown-watch".to_string(),
            trigger_spec: json!({ "event": "attention", "repo": repo_root }).to_string(),
            scope: json!({ "attention": "!ok" }).to_string(),
            program: "builtin:status".to_string(),
            capabilities: vec!["observe".to_string(), "mark".to_string()],
            cooldown_secs: 3600,
            ..Default::default()
        },
    )
    .await;

    let in_flight = overlooker::new_in_flight();
    let ev = events::Event {
        id: 0,
        branch_id: branch_id.clone(),
        kind: "attention".to_string(),
        data: json!({ "level": "blocked" }),
        created_at: db::now_iso(),
    };

    // First reactive fire runs; it stamps `last_run_at`.
    overlooker::dispatch(&state, &in_flight, &ev).await;
    // Second reactive fire inside the cooldown gap is recorded `skipped`.
    overlooker::dispatch(&state, &in_flight, &ev).await;

    let runs = ov::recent_runs(&state.db, &o.id, 10).await.unwrap();
    assert_eq!(runs.len(), 2, "both fires produce a run row");
    assert_eq!(runs[0].outcome, "skipped", "the second fire is on cooldown");
    assert!(
        runs[0].summary.contains("cooldown"),
        "the skip says why: {}",
        runs[0].summary
    );
    assert_eq!(runs[1].outcome, "ok", "the first fire ran");

    // No-overlap: with the overlooker's id already in the in-flight set, a fire
    // is dropped (no new run row) — a round of it is conceptually already
    // running.
    {
        let set = overlooker::new_in_flight();
        set.lock().await.insert(o.id.clone());
        let before = ov::recent_runs(&state.db, &o.id, 100).await.unwrap().len();
        let dropped = overlooker::fire(&state, &set, &o, "event:attention", false).await;
        assert!(dropped.is_none(), "an in-flight re-fire is dropped");
        let after = ov::recent_runs(&state.db, &o.id, 100).await.unwrap().len();
        assert_eq!(before, after, "a dropped re-fire opens no run row");
    }

    ts.client
        .delete(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
}

/// T8/T9: the operator REST surface end to end — create via POST, read back via
/// GET, enable via PATCH, fire a `dry_run` round via POST /run (the audit row
/// comes back with an outcome), list the round history via GET /runs, then
/// DELETE. Plus the validation gates: a bad capability and a duplicate name are
/// both rejected.
#[serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rest_overlooker_lifecycle_and_validation() {
    let ts = TestServer::start().await;
    // A non-ok session so the dry-run round has something in scope to "would-mark".
    let (session_id, _branch_id, _repo_root) = make_session(&ts, "rest me").await;
    ts.client
        .patch(
            &format!("/api/sessions/{session_id}"),
            json!({ "attention": "attention" }),
        )
        .await
        .unwrap();

    // Create: structured trigger/scope/params JSON in, an OverlookerView out.
    let created = ts
        .client
        .post(
            "/api/overlookers",
            json!({
                "name": "rest-watch",
                "trigger": { "cron": "0 * * * *" },
                "scope": { "attention": "!ok" },
                "program": "builtin:status",
                "params": { "prompt": "is it stuck?" },
                "capabilities": ["observe", "mark"],
            }),
        )
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["name"], "rest-watch");
    assert_eq!(created["enabled"], false, "new overlookers start disabled");
    // JSON-bearing fields come back parsed, not as strings.
    assert_eq!(created["trigger"]["cron"], "0 * * * *");
    assert_eq!(created["scope"]["attention"], "!ok");
    assert_eq!(created["params"]["prompt"], "is it stuck?");
    assert_eq!(created["capabilities"][1], "mark");
    assert!(created["last_outcome"].is_null(), "never run yet");

    // A duplicate name is rejected (the client surfaces the error status).
    let dup = ts
        .client
        .post("/api/overlookers", json!({ "name": "rest-watch" }))
        .await;
    assert!(dup.is_err(), "a duplicate name is rejected");

    // A bad capability is rejected at create time.
    let bad_cap = ts
        .client
        .post(
            "/api/overlookers",
            json!({ "name": "bad", "capabilities": ["observe", "teleport"] }),
        )
        .await;
    assert!(bad_cap.is_err(), "an unknown capability is rejected");

    // GET by id (resolve also accepts the name).
    let got = ts
        .client
        .get(&format!("/api/overlookers/{id}"))
        .await
        .unwrap();
    assert_eq!(got["name"], "rest-watch");
    let by_name = ts.client.get("/api/overlookers/rest-watch").await.unwrap();
    assert_eq!(by_name["id"], id.as_str());

    // It shows up in the list.
    let list = ts.client.get("/api/overlookers").await.unwrap();
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["id"] == id.as_str()));

    // Enable via PATCH, and update a mutable field in the same call.
    let patched = ts
        .client
        .patch(
            &format!("/api/overlookers/{id}"),
            json!({ "enabled": true, "cooldown_secs": 120 }),
        )
        .await
        .unwrap();
    assert_eq!(patched["enabled"], true);
    assert_eq!(patched["cooldown_secs"], 120, "a mutable field updates");

    // Dry-run a round: it returns a run id + outcome, applies no mark.
    let run = ts
        .client
        .post(
            &format!("/api/overlookers/{id}/run"),
            json!({ "dry_run": true }),
        )
        .await
        .unwrap();
    let run_id = run["run_id"].as_i64().unwrap();
    assert!(run_id > 0, "a run row was opened");
    assert_eq!(
        run["outcome"], "ok",
        "the round surveyed the scoped session"
    );
    let view = ts
        .client
        .get(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
    assert_eq!(
        view["branch"]["triage_level"], "",
        "a dry run applies no mark"
    );

    // GET /runs returns the audit history with actions parsed back to JSON.
    let runs = ts
        .client
        .get(&format!("/api/overlookers/{id}/runs?limit=10"))
        .await
        .unwrap();
    let runs = runs.as_array().unwrap();
    assert!(!runs.is_empty(), "the round is in the history");
    assert_eq!(runs[0]["id"], run_id);
    assert!(
        runs[0]["actions"].is_array(),
        "actions come back as parsed JSON"
    );

    // DELETE.
    let deleted = ts
        .client
        .delete(&format!("/api/overlookers/{id}"))
        .await
        .unwrap();
    assert_eq!(deleted["deleted"], true);
    let gone = ts.client.get(&format!("/api/overlookers/{id}")).await;
    assert!(gone.is_err(), "a deleted overlooker 404s");

    ts.client
        .delete(&format!("/api/sessions/{session_id}"))
        .await
        .unwrap();
}
