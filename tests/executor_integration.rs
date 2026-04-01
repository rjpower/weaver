use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use weaver::{
    create_issue, get_issue, get_result_comment, update_issue, AgentRunner, CreateIssueParams,
    Executor, ExecutorConfig, IssueStatus, UpdateIssueParams,
};

fn mock_runner(api_url: &str) -> Arc<AgentRunner> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Arc::new(AgentRunner {
        api_url: api_url.into(),
        workflows_dir: manifest_dir.join("skills/builtins"),
        sdk_dir: PathBuf::from("/nonexistent"),
        binary: manifest_dir
            .join("tests/mock_agent.sh")
            .to_string_lossy()
            .into(),
    })
}

async fn test_db() -> weaver::Db {
    weaver::db::connect_in_memory().await.unwrap()
}

#[tokio::test]
async fn create_and_run_single_issue() {
    let db = test_db().await;

    let issue = create_issue(
        &db,
        CreateIssueParams {
            title: "single issue".into(),
            body: Some("hello world".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let runner = mock_runner("http://localhost:0");
    let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);
    let report = executor.run_once().await.unwrap();
    assert_eq!(report.executed.len(), 1);

    let after = get_issue(&db, &issue.id).await.unwrap();
    assert_eq!(after.status, IssueStatus::Completed);
    let result_text = get_result_comment(&db, &after.id).await.unwrap().unwrap_or_default();
    assert!(
        result_text.contains("mock:"),
        "Expected mock result, got: {result_text}"
    );
}

#[tokio::test]
async fn run_issue_with_deps() {
    let db = test_db().await;

    let a = create_issue(
        &db,
        CreateIssueParams {
            title: "A".into(),
            body: Some("task A".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let b = create_issue(
        &db,
        CreateIssueParams {
            title: "B".into(),
            body: Some("task B".into()),
            dependencies: vec![a.id.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let c = create_issue(
        &db,
        CreateIssueParams {
            title: "C".into(),
            body: Some("task C".into()),
            dependencies: vec![a.id.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let runner = mock_runner("http://localhost:0");
    let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);

    // First pass: only A is ready (B and C depend on it)
    let report = executor.run_once().await.unwrap();
    assert_eq!(report.completed.len(), 1);
    assert_eq!(
        get_issue(&db, &a.id).await.unwrap().status,
        IssueStatus::Completed
    );

    // Second pass: B and C are now ready
    let report = executor.run_once().await.unwrap();
    assert_eq!(report.completed.len(), 2);

    assert_eq!(
        get_issue(&db, &b.id).await.unwrap().status,
        IssueStatus::Completed
    );
    assert_eq!(
        get_issue(&db, &c.id).await.unwrap().status,
        IssueStatus::Completed
    );
}

#[tokio::test]
async fn dep_failure_blocks_dependents() {
    let db = test_db().await;

    let a = create_issue(
        &db,
        CreateIssueParams {
            title: "A".into(),
            body: Some("task A".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let b = create_issue(
        &db,
        CreateIssueParams {
            title: "B".into(),
            body: Some("task B".into()),
            dependencies: vec![a.id.clone()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Mark A as failed
    update_issue(
        &db,
        &a.id,
        UpdateIssueParams {
            status: Some(IssueStatus::Failed),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let runner = mock_runner("http://localhost:0");
    let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);

    let report = executor.run_once().await.unwrap();
    // B should not have been executed
    assert!(report.executed.is_empty());

    let b_after = get_issue(&db, &b.id).await.unwrap();
    assert_eq!(b_after.status, IssueStatus::Blocked);
}

#[tokio::test]
async fn cancel_pending_issue() {
    let db = test_db().await;

    let issue = create_issue(
        &db,
        CreateIssueParams {
            title: "will cancel".into(),

            ..Default::default()
        },
    )
    .await
    .unwrap();

    let updated = update_issue(
        &db,
        &issue.id,
        UpdateIssueParams {
            status: Some(IssueStatus::Failed),
            error: Some("Cancelled by user".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.status, IssueStatus::Failed);
    assert_eq!(updated.error.as_deref(), Some("Cancelled by user"));
}

#[tokio::test]
async fn run_once_executes_all_ready() {
    let db = test_db().await;

    for i in 0..3 {
        create_issue(
            &db,
            CreateIssueParams {
                title: format!("issue {i}"),
                body: Some(format!("task {i}")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let runner = mock_runner("http://localhost:0");
    let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);

    let report = executor.run_once().await.unwrap();
    assert_eq!(report.completed.len(), 3);
}

#[tokio::test]
async fn priority_ordering() {
    let db = test_db().await;

    create_issue(
        &db,
        CreateIssueParams {
            title: "low priority".into(),
            body: Some("low".into()),
            priority: 0,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    create_issue(
        &db,
        CreateIssueParams {
            title: "high priority".into(),
            body: Some("high".into()),
            priority: 10,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let runner = mock_runner("http://localhost:0");
    let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);

    let report = executor.run_once().await.unwrap();
    assert_eq!(report.completed.len(), 2);
}

#[tokio::test]
async fn parent_child_relationship() {
    let db = test_db().await;

    let parent = create_issue(
        &db,
        CreateIssueParams {
            title: "parent".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let child = create_issue(
        &db,
        CreateIssueParams {
            title: "child".into(),
            parent_issue_id: Some(parent.id.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let fetched = get_issue(&db, &child.id).await.unwrap();
    assert_eq!(fetched.parent_issue_id.as_deref(), Some(parent.id.as_str()));
}

#[tokio::test]
async fn auto_commit_on_completion() {
    let db = test_db().await;

    // Set up a temp git repo as the work_dir
    let tmp = tempfile::tempdir().unwrap();
    let work_dir = tmp.path();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(work_dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial"])
        .current_dir(work_dir)
        .output()
        .unwrap();

    let context = json!({ "work_dir": work_dir.to_str().unwrap() });
    create_issue(
        &db,
        CreateIssueParams {
            title: "auto-commit test".into(),
            body: Some("test body".into()),
            context: Some(context),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Use the mock agent that creates an uncommitted file
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runner = Arc::new(AgentRunner {
        api_url: "http://localhost:0".into(),
        workflows_dir: manifest_dir.join("skills/builtins"),
        sdk_dir: PathBuf::from("/nonexistent"),
        binary: manifest_dir
            .join("tests/mock_agent_with_file.sh")
            .to_string_lossy()
            .into(),
    });

    let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);
    let report = executor.run_once().await.unwrap();
    assert_eq!(report.completed.len(), 1);

    // The file should exist and be committed
    assert!(work_dir.join("uncommitted_file.txt").exists());

    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(work_dir)
        .output()
        .unwrap();
    let last_commit = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        last_commit.contains("wip:"),
        "Expected auto-commit with wip: prefix, got: {last_commit}"
    );

    // Verify no uncommitted changes remain
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(work_dir)
        .output()
        .unwrap();
    let status = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        status.trim().is_empty(),
        "Expected clean worktree, got: {status}"
    );
}
