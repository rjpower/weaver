use std::path::PathBuf;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use weaver::{
    create_issue, AgentRunner, CreateIssueParams, Executor, ExecutorConfig, IssueStatus,
};

async fn test_db() -> weaver::Db {
    weaver::db::connect_in_memory().await.unwrap()
}

fn test_runner(api_url: &str) -> Arc<AgentRunner> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Arc::new(AgentRunner {
        api_url: api_url.into(),
        workflows_dir: manifest_dir.join("tests"),
        sdk_dir: PathBuf::from("/nonexistent"),
        binary: manifest_dir
            .join("tests/mock_agent.sh")
            .to_string_lossy()
            .into(),
    })
}

#[tokio::test]
#[ignore] // Requires uv installed and network access for pip
async fn workflow_with_ephemeral_api() {
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

    let parent = create_issue(
        &db,
        CreateIssueParams {
            title: "Test workflow".into(),
            body: Some("run the test workflow".into()),
            tags: vec!["test_workflow".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let runner = test_runner(&api_url);
    let executor = Executor::new(db.clone(), ExecutorConfig::default(), runner.clone());

    let loop_cancel = CancellationToken::new();
    let loop_cancel_clone = loop_cancel.clone();
    let loop_executor = Executor::new(db.clone(), ExecutorConfig::default(), runner);
    tokio::spawn(async move {
        if let Err(e) = loop_executor.run_loop(loop_cancel_clone).await {
            tracing::error!(error = %e, "Executor loop failed");
        }
    });

    let parent_after = executor
        .wait_for_issue(&parent.id, CancellationToken::new())
        .await
        .unwrap();
    assert!(
        parent_after.status == IssueStatus::Completed
            || parent_after.status == IssueStatus::Failed,
        "Expected terminal status, got {:?}",
        parent_after.status
    );

    loop_cancel.cancel();
    cancel.cancel();
}
