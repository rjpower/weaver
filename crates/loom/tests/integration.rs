//! End-to-end test driving a real server with a shell-backed session.
//! Requires `git` and `tmux` on PATH.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::json;
use tokio::net::TcpListener;
use loom::client::Client;
use loom::events::EventBus;
use loom::web::AppState;
use loom::{db, server, tmux};

fn sh(dir: &Path, program: &str, args: &[&str]) {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {program}: {e}"));
    assert!(status.success(), "{program} {args:?} failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_lifecycle() {
    // Isolate all weaver state in a temp dir.
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("WEAVER_HOME", home.path());

    // Build a throwaway git repo with a single commit on `main`.
    let repo = tempfile::tempdir().unwrap();
    sh(repo.path(), "git", &["init", "-b", "main"]);
    sh(repo.path(), "git", &["config", "user.email", "t@t.test"]);
    sh(repo.path(), "git", &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("README.md"), "hello\n").unwrap();
    sh(repo.path(), "git", &["add", "."]);
    sh(repo.path(), "git", &["commit", "-m", "init"]);

    // Start the server on a random port.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let pool = db::connect(&db::default_db_path()).await.unwrap();
    let state = AppState {
        db: pool.clone(),
        bus: EventBus::new(),
        addr: addr.to_string(),
    };
    tokio::spawn(server::serve(state, listener));

    std::env::set_var("WEAVER_API", format!("http://{addr}"));
    let client = Client::new();
    for _ in 0..60 {
        if client.get("/api/health").await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Create a session backed by a plain shell.
    let ws = client
        .post(
            "/api/sessions",
            json!({
                "goal": "integration test goal",
                "cwd": repo.path().to_string_lossy(),
                "agent": "shell",
            }),
        )
        .await
        .unwrap();
    let id = ws["id"].as_str().unwrap().to_string();
    let session = ws["tmux_session"].as_str().unwrap().to_string();
    let work_dir = ws["work_dir"].as_str().unwrap().to_string();
    let repo_root = ws["branch"]["repo_root"].as_str().unwrap().to_string();

    assert!(
        std::path::Path::new(&work_dir).join(".git").exists(),
        "worktree was not created"
    );
    assert!(
        work_dir.ends_with("/.worktrees/integration-test-goal"),
        "worktree should live inside the repo at .worktrees/<slug>, got {work_dir}"
    );
    assert_eq!(ws["branch"]["branch"], "weaver/integration-test-goal");
    assert_eq!(
        ws["branch"]["title"], "integration test goal",
        "title derived from goal"
    );
    assert!(tmux::has_session(&session).await, "tmux session missing");

    let list = client.get("/api/sessions").await.unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    let recent = client.get("/api/repos/recent").await.unwrap();
    let recent = recent.as_array().unwrap();
    assert_eq!(recent.len(), 1, "repo should be recorded after first session");
    assert_eq!(recent[0]["repo_root"], repo_root);
    assert_eq!(recent[0]["active_branches"], 1);

    // Text sent to the session reaches the pane.
    client
        .post(
            &format!("/api/sessions/{id}/send"),
            json!({ "text": "echo WEAVER_MARKER_123" }),
        )
        .await
        .unwrap();
    let mut found = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let pane = client
            .get(&format!("/api/sessions/{id}/pane"))
            .await
            .unwrap();
        if pane["content"]
            .as_str()
            .unwrap_or("")
            .contains("WEAVER_MARKER_123")
        {
            found = true;
            break;
        }
    }
    assert!(found, "sent text never appeared in the pane");

    // Interrupting the agent sends an Esc keypress and leaves the session up.
    let res = client
        .post(&format!("/api/sessions/{id}/interrupt"), json!({}))
        .await
        .unwrap();
    assert_eq!(res["interrupted"], true);
    assert!(
        tmux::has_session(&session).await,
        "interrupt should not kill the tmux session"
    );

    // A hook flips the session status. The monitor consumes new `hook`
    // event rows on its next tick.
    let branch_id = {
        let s = loom::session::get(&pool, &id).await.unwrap().unwrap();
        s.branch_id
    };
    weaver_core::events::record_local(
        &pool,
        &branch_id,
        "hook",
        json!({ "event": "working" }),
    )
    .await
    .unwrap();
    let mut working = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let ws = client.get(&format!("/api/sessions/{id}")).await.unwrap();
        if ws["status"] == "working" {
            working = true;
            break;
        }
    }
    assert!(working, "monitor should have flipped status to working");

    // The diff endpoint responds (no changes yet).
    let diff = client
        .get(&format!("/api/sessions/{id}/diff"))
        .await
        .unwrap();
    assert!(diff["patch"].is_string());

    // Branches endpoint lists this branch with the right metadata.
    let branches = client.get("/api/branches").await.unwrap();
    let arr = branches.as_array().unwrap();
    assert_eq!(arr.len(), 1, "one branch tracked");
    assert_eq!(arr[0]["branch"], "weaver/integration-test-goal");
    assert_eq!(arr[0]["open_issue_count"], 0);

    // Issue CRUD lives at /api/branches/{id}/issues + /api/issues/{id}.
    let created = client
        .post(
            &format!("/api/branches/{branch_id}/issues"),
            json!({ "title": "fix it", "body": "details" }),
        )
        .await
        .unwrap();
    let issue_id = created["id"].as_i64().unwrap();
    assert_eq!(created["status"], "open");
    let listed = client
        .get(&format!("/api/branches/{branch_id}/issues"))
        .await
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let branch_view = client
        .get(&format!("/api/branches/{branch_id}"))
        .await
        .unwrap();
    assert_eq!(branch_view["open_issue_count"], 1);
    let _ = client
        .delete(&format!("/api/issues/{issue_id}"))
        .await
        .unwrap();

    // ---- Attach to an existing branch -----------------------------------
    let branches_q = client
        .get(&format!(
            "/api/repos/branches?cwd={}",
            repo.path().to_string_lossy()
        ))
        .await
        .unwrap();
    let arr = branches_q.as_array().unwrap();
    assert!(
        arr.iter().any(|b| b["name"] == "main" && b["current"] == true),
        "main should be listed as current, got {arr:?}"
    );

    sh(repo.path(), "git", &["branch", "feature/x", "main"]);
    let attached = client
        .post(
            "/api/sessions",
            json!({
                "cwd": repo.path().to_string_lossy(),
                "goal": "attach to feature/x",
                "agent": "shell",
                "existing_branch": "feature/x",
            }),
        )
        .await
        .unwrap();
    assert_eq!(attached["branch"]["branch"], "feature/x");
    let attached_id = attached["id"].as_str().unwrap().to_string();
    let attached_dir = attached["work_dir"].as_str().unwrap().to_string();
    assert!(
        attached_dir.ends_with("/.worktrees/feature-x"),
        "attached worktree should live at .worktrees/feature-x, got {attached_dir}"
    );
    assert!(std::path::Path::new(&attached_dir).join(".git").exists());

    sh(repo.path(), "git", &["branch", "feature/y", "main"]);
    let preexisting = repo.path().join("custom-worktree-y");
    sh(
        repo.path(),
        "git",
        &[
            "worktree",
            "add",
            preexisting.to_str().unwrap(),
            "feature/y",
        ],
    );
    let attached_y = client
        .post(
            "/api/sessions",
            json!({
                "cwd": repo.path().to_string_lossy(),
                "goal": "attach to feature/y",
                "agent": "shell",
                "existing_branch": "feature/y",
            }),
        )
        .await
        .unwrap();
    assert_eq!(attached_y["branch"]["branch"], "feature/y");
    let dir_y = attached_y["work_dir"].as_str().unwrap().to_string();
    assert_eq!(
        std::fs::canonicalize(&dir_y).unwrap(),
        std::fs::canonicalize(&preexisting).unwrap(),
        "weaver should reuse the pre-existing worktree path"
    );

    let missing = client
        .post(
            "/api/sessions",
            json!({
                "cwd": repo.path().to_string_lossy(),
                "goal": "missing branch",
                "agent": "shell",
                "existing_branch": "no/such/branch",
            }),
        )
        .await;
    assert!(missing.is_err(), "missing branch should be rejected");

    client
        .delete(&format!("/api/sessions/{attached_id}"))
        .await
        .unwrap();
    client
        .delete(&format!(
            "/api/sessions/{}",
            attached_y["id"].as_str().unwrap()
        ))
        .await
        .unwrap();

    // Adoption.
    tmux::kill_session(&session).await.unwrap();
    assert!(
        !tmux::has_session(&session).await,
        "session should be gone after kill"
    );
    let adopted = client
        .post(&format!("/api/sessions/{id}/adopt"), json!({}))
        .await
        .unwrap();
    assert_eq!(adopted["status"], "launching", "adopt sets status launching");
    assert!(
        tmux::has_session(&session).await,
        "adopt should recreate the tmux session"
    );
    assert!(
        client
            .post(&format!("/api/sessions/{id}/adopt"), json!({}))
            .await
            .is_err(),
        "adopting a live session should fail"
    );

    // A session can be created with no goal at all.
    let bare = client
        .post(
            "/api/sessions",
            json!({
                "cwd": repo.path().to_string_lossy(),
                "title": "no goal here",
                "agent": "shell",
            }),
        )
        .await
        .unwrap();
    assert_eq!(bare["branch"]["goal"], "", "goal should be empty");
    assert_eq!(bare["branch"]["title"], "no goal here");
    let bare_id = bare["id"].as_str().unwrap().to_string();
    client
        .delete(&format!("/api/sessions/{bare_id}"))
        .await
        .unwrap();

    // Deleting the session tears down the tmux session and the DB row.
    client
        .delete(&format!("/api/sessions/{id}"))
        .await
        .unwrap();
    assert!(
        !tmux::has_session(&session).await,
        "tmux session was not killed"
    );
    let list = client.get("/api/sessions").await.unwrap();
    assert_eq!(list.as_array().unwrap().len(), 0);

    let recent = client.get("/api/repos/recent").await.unwrap();
    let recent = recent.as_array().unwrap();
    assert_eq!(recent.len(), 1, "recent repo should outlive its sessions");
    assert_eq!(recent[0]["repo_root"], repo_root);
    assert_eq!(recent[0]["active_branches"], 0);
}
