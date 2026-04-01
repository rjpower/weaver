use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use weaver::{
    create_issue, get_result_comment, list_issues, AgentRunner, CreateIssueParams, Executor,
    ExecutorConfig, IssueScope, IssueStatus, ListFilter,
};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mock_binary() -> String {
    env!("CARGO_BIN_EXE_mock_agent").to_string()
}

fn smart_runner(api_url: &str) -> Arc<AgentRunner> {
    Arc::new(AgentRunner {
        api_url: api_url.into(),
        workflows_dir: manifest_dir().join("skills/builtins"),
        sdk_dir: PathBuf::from("/nonexistent"),
        binary: mock_binary(),
    })
}

fn leaf_runner(api_url: &str) -> Arc<AgentRunner> {
    Arc::new(AgentRunner {
        api_url: api_url.into(),
        workflows_dir: manifest_dir().join("skills/builtins"),
        sdk_dir: PathBuf::from("/nonexistent"),
        binary: manifest_dir()
            .join("tests/mock_agent.sh")
            .to_string_lossy()
            .into(),
    })
}

async fn test_db() -> weaver::Db {
    weaver::db::connect_in_memory().await.unwrap()
}

struct TestHarness {
    db: weaver::Db,
    api_url: String,
    work_dir: tempfile::TempDir,
    _server_cancel: CancellationToken,
}

impl TestHarness {
    async fn new() -> Self {
        let db = test_db().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let api_url = format!("http://{addr}");
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let server_db = db.clone();
        tokio::spawn(async move {
            weaver::serve(server_db, listener, server_cancel)
                .await
                .unwrap();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Isolated git repo so tests never touch the real repo
        let work_dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(work_dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "initial"])
            .current_dir(work_dir.path())
            .output()
            .unwrap();

        Self {
            db,
            api_url,
            work_dir,
            _server_cancel: cancel,
        }
    }

    fn work_dir_context(&self) -> serde_json::Value {
        serde_json::json!({ "work_dir": self.work_dir.path().to_str().unwrap() })
    }

    /// Run the executor loop in background and wait for a specific issue to complete.
    async fn run_until_done(&self, issue_id: &str, runner: Arc<AgentRunner>) -> weaver::Issue {
        let loop_cancel = CancellationToken::new();
        let loop_cancel_clone = loop_cancel.clone();
        let loop_executor = Executor::new(
            self.db.clone(),
            ExecutorConfig {
                poll_interval_secs: 1,
                timeout_secs: 60,
                ..Default::default()
            },
            runner.clone(),
        );
        tokio::spawn(async move {
            loop_executor.run_loop(loop_cancel_clone).await.ok();
        });

        let wait_executor = Executor::new(
            self.db.clone(),
            ExecutorConfig {
                poll_interval_secs: 1,
                timeout_secs: 60,
                ..Default::default()
            },
            runner,
        );
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            wait_executor.wait_for_issue(issue_id, CancellationToken::new()),
        )
        .await
        .expect("timed out waiting for issue")
        .expect("wait_for_issue failed");

        loop_cancel.cancel();
        result
    }
}

// ---------------------------------------------------------------------------
// Test: Skill template is resolved and expanded into the system prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn skill_template_expansion_in_agent_call() {
    let h = TestHarness::new().await;
    let runner = leaf_runner(&h.api_url);

    let issue = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Test design".into(),
            body: Some("Build a widget".into()),
            tags: vec!["design".into()],
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&issue.id, runner).await;
    assert_eq!(result.status, IssueStatus::Completed);
    let result_text = get_result_comment(&h.db, &result.id).await.unwrap().unwrap_or_default();
    assert!(
        result_text.contains("Test design"),
        "Expected prompt to contain issue title, got: {result_text}"
    );
}

// ---------------------------------------------------------------------------
// Test: Issue without skill tag executes as direct agent call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn issue_without_skill_tag_runs_direct() {
    let h = TestHarness::new().await;
    let runner = leaf_runner(&h.api_url);

    let issue = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Plain task".into(),
            body: Some("Do something simple".into()),
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&issue.id, runner).await;
    assert_eq!(result.status, IssueStatus::Completed);
    assert!(get_result_comment(&h.db, &result.id).await.unwrap().unwrap_or_default().contains("mock:"));
}

// ---------------------------------------------------------------------------
// Test: Coordinator creates a single child issue via API, child completes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coordinator_creates_child_issue() {
    let h = TestHarness::new().await;
    let runner = smart_runner(&h.api_url);

    let parent = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Parent coordinator".into(),
            body: Some("MOCK_CREATE_CHILD:subtask:do the thing".into()),
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&parent.id, runner).await;
    assert_eq!(
        result.status,
        IssueStatus::Completed,
        "Parent should complete. Error: {:?}",
        result.error,
    );

    let result_text = get_result_comment(&h.db, &result.id).await.unwrap().unwrap_or_default();
    assert!(
        result_text.contains("coordinator: child"),
        "Expected coordinator result, got: {result_text}"
    );

    // Verify child issue was created and completed
    let children = list_issues(
        &h.db,
        ListFilter {
            scope: IssueScope::ChildrenOf(parent.id.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .issues;

    assert_eq!(children.len(), 1, "Expected 1 child issue");
    assert_eq!(children[0].title, "subtask");
    assert_eq!(children[0].status, IssueStatus::Completed);
    assert!(children[0].tags.contains(&"step".to_string()));
}

// ---------------------------------------------------------------------------
// Test: Coordinator creates multiple parallel child issues
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coordinator_creates_parallel_children() {
    let h = TestHarness::new().await;
    let runner = smart_runner(&h.api_url);

    let parent = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Parallel coordinator".into(),
            body: Some("MOCK_CREATE_CHILDREN:3".into()),
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&parent.id, runner).await;
    assert_eq!(
        result.status,
        IssueStatus::Completed,
        "Parent should complete. Error: {:?}",
        result.error,
    );

    let result_text = get_result_comment(&h.db, &result.id).await.unwrap().unwrap_or_default();
    assert!(
        result_text.contains("all 3 children completed"),
        "Expected all children completed, got: {result_text}"
    );

    let children = list_issues(
        &h.db,
        ListFilter {
            scope: IssueScope::ChildrenOf(parent.id.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .issues;

    assert_eq!(children.len(), 3, "Expected 3 child issues");
    for child in &children {
        assert_eq!(
            child.status,
            IssueStatus::Completed,
            "Child '{}' should be completed, got {:?}",
            child.title,
            child.status
        );
    }
}

// ---------------------------------------------------------------------------
// Test: Coordinator creates a dependency chain (A → B → C)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coordinator_creates_dependency_chain() {
    let h = TestHarness::new().await;
    let runner = smart_runner(&h.api_url);

    let parent = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Chain coordinator".into(),
            body: Some("MOCK_CREATE_CHAIN:3".into()),
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&parent.id, runner).await;
    assert_eq!(
        result.status,
        IssueStatus::Completed,
        "Parent should complete. Error: {:?}",
        result.error,
    );

    let result_text = get_result_comment(&h.db, &result.id).await.unwrap().unwrap_or_default();
    assert!(
        result_text.contains("chain of 3 completed"),
        "Expected chain completed, got: {result_text}"
    );

    let children = list_issues(
        &h.db,
        ListFilter {
            scope: IssueScope::ChildrenOf(parent.id.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .issues;

    assert_eq!(children.len(), 3, "Expected 3 chain issues");
    for child in &children {
        assert_eq!(
            child.status,
            IssueStatus::Completed,
            "Chain issue '{}' should be completed",
            child.title
        );
    }

    // Verify dependency chain structure
    let chain_1 = children.iter().find(|c| c.title == "chain-1").unwrap();
    let chain_2 = children.iter().find(|c| c.title == "chain-2").unwrap();
    let chain_3 = children.iter().find(|c| c.title == "chain-3").unwrap();

    assert!(chain_1.dependencies.is_empty());
    assert_eq!(chain_2.dependencies, vec![chain_1.id.clone()]);
    assert_eq!(chain_3.dependencies, vec![chain_2.id.clone()]);
}

// ---------------------------------------------------------------------------
// Test: Agent failure propagates correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_failure_marks_issue_failed() {
    let h = TestHarness::new().await;
    let runner = smart_runner(&h.api_url);

    let issue = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Will fail".into(),
            body: Some("MOCK_FAIL".into()),
            max_tries: Some(1),
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&issue.id, runner).await;
    assert_eq!(result.status, IssueStatus::Failed);
    assert!(result.error.is_some());
}

// ---------------------------------------------------------------------------
// Test: Explicit result from mock agent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mock_result_directive() {
    let h = TestHarness::new().await;
    let runner = smart_runner(&h.api_url);

    let issue = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Custom result".into(),
            body: Some("MOCK_RESULT:the answer is 42".into()),
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&issue.id, runner).await;
    assert_eq!(result.status, IssueStatus::Completed);
    let result_text = get_result_comment(&h.db, &result.id).await.unwrap();
    assert_eq!(result_text.as_deref(), Some("the answer is 42"));
}

// ---------------------------------------------------------------------------
// Test: Skill-tagged coordinator creates child, child also completes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nested_skill_execution() {
    let h = TestHarness::new().await;
    let runner = smart_runner(&h.api_url);

    let parent = create_issue(
        &h.db,
        CreateIssueParams {
            title: "Nested skills".into(),
            body: Some("MOCK_CREATE_CHILD:inner-task:do nested work".into()),
            tags: vec!["design".into()],
            context: Some(h.work_dir_context()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = h.run_until_done(&parent.id, runner).await;
    assert_eq!(
        result.status,
        IssueStatus::Completed,
        "Parent should complete. Error: {:?}",
        result.error,
    );

    let children = list_issues(
        &h.db,
        ListFilter {
            scope: IssueScope::ChildrenOf(parent.id.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .issues;

    assert_eq!(children.len(), 1);
    assert_eq!(children[0].status, IssueStatus::Completed);
}
