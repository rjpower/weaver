/// Mock Claude agent for integration tests.
///
/// Behavior is controlled by the prompt content:
/// - `MOCK_FAIL` → exit 1
/// - `MOCK_RESULT:<text>` → return that text as result
/// - `MOCK_CREATE_CHILD:<title>:<body>` → create child issue via API, wait for completion
/// - `MOCK_CREATE_CHILDREN:<n>` → create N parallel child issues, wait for all
/// - `MOCK_CREATE_CHAIN:<n>` → create N issues in a dependency chain, wait for last
/// - Otherwise → echo prompt back as result
///
/// Reads `WEAVER_API_URL` and `WEAVER_ISSUE_ID` from environment.
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut prompt = String::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--system-prompt" | "--output-format" | "--model" | "--resume" => {
                i += 1; // skip value
            }
            "--print" | "--dangerously-skip-permissions" | "--verbose" => {}
            _ => {
                prompt = args[i].clone();
            }
        }
        i += 1;
    }

    let api_url = std::env::var("WEAVER_API_URL").unwrap_or_default();
    let parent_id = std::env::var("WEAVER_ISSUE_ID").unwrap_or_default();

    if prompt.contains("MOCK_FAIL") {
        eprintln!("Mock agent failing as requested");
        std::process::exit(1);
    }

    if let Some(text) = extract_directive(&prompt, "MOCK_RESULT:") {
        print_result(&text);
        return;
    }

    if let Some(directive) = extract_directive(&prompt, "MOCK_CREATE_CHILD:") {
        let parts: Vec<&str> = directive.splitn(2, ':').collect();
        let title = parts[0];
        let body = parts.get(1).unwrap_or(&"child task");

        match create_child(&api_url, title, body, &parent_id) {
            Some(id) => {
                wait_for_terminal(&api_url, &id);
                let child_result = get_result(&api_url, &id);
                print_result(&format!(
                    "coordinator: child {id} completed with: {child_result}"
                ));
            }
            None => {
                print_result("error: failed to create child issue");
            }
        }
        return;
    }

    if let Some(n_str) = extract_directive(&prompt, "MOCK_CREATE_CHILDREN:") {
        let n: usize = n_str.trim().parse().unwrap_or(1);
        let mut child_ids = Vec::new();

        for i in 1..=n {
            if let Some(id) = create_child(
                &api_url,
                &format!("child-{i}"),
                &format!("parallel task {i}"),
                &parent_id,
            ) {
                child_ids.push(id);
            }
        }

        for id in &child_ids {
            wait_for_terminal(&api_url, id);
        }

        print_result(&format!("coordinator: all {n} children completed"));
        return;
    }

    if let Some(n_str) = extract_directive(&prompt, "MOCK_CREATE_CHAIN:") {
        let n: usize = n_str.trim().parse().unwrap_or(1);
        let mut prev_id: Option<String> = None;
        let mut last_id = String::new();

        for i in 1..=n {
            let payload = serde_json::json!({
                "title": format!("chain-{i}"),
                "body": format!("chain task {i}"),
                "tags": ["step"],
                "parent_issue_id": parent_id,
                "dependencies": prev_id.iter().cloned().collect::<Vec<_>>(),
            });

            if let Some(id) = post_issue(&api_url, &payload) {
                last_id = id.clone();
                prev_id = Some(id);
            }
        }

        if !last_id.is_empty() {
            wait_for_terminal(&api_url, &last_id);
        }

        print_result(&format!("coordinator: chain of {n} completed"));
        return;
    }

    // Default: echo prompt back
    let escaped = prompt
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");
    print_result(&format!("mock: {escaped}"));
}

fn print_result(result: &str) {
    // Stream-json NDJSON format: init, assistant, result
    let init = serde_json::json!({
        "type": "system",
        "subtype": "init",
        "session_id": "mock-session",
        "model": "mock-model",
        "tools": []
    });
    println!("{init}");

    let assistant = serde_json::json!({
        "type": "assistant",
        "message": {
            "content": [{"type": "text", "text": result}]
        },
        "session_id": "mock-session"
    });
    println!("{assistant}");

    let output = serde_json::json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": result,
        "session_id": "mock-session",
        "total_cost_usd": 0.01,
        "model": "mock-model",
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50
        }
    });
    println!("{output}");
}

fn extract_directive(prompt: &str, prefix: &str) -> Option<String> {
    let start = prompt.find(prefix)?;
    let after = &prompt[start + prefix.len()..];
    let end = after
        .find(|c: char| c == '\n' || c == '"')
        .unwrap_or(after.len());
    Some(after[..end].to_string())
}

fn create_child(api_url: &str, title: &str, body: &str, parent_id: &str) -> Option<String> {
    let payload = serde_json::json!({
        "title": title,
        "body": body,
        "tags": ["step"],
        "parent_issue_id": parent_id,
    });
    post_issue(api_url, &payload)
}

fn post_issue(api_url: &str, payload: &serde_json::Value) -> Option<String> {
    let url = format!("{api_url}/api/issues");
    let body = serde_json::to_string(payload).ok()?;

    let output = std::process::Command::new("curl")
        .args([
            "-s", "-X", "POST", &url, "-H",
            "Content-Type: application/json", "-d", &body,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let response: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    response
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_issue_json(api_url: &str, issue_id: &str) -> Option<serde_json::Value> {
    let url = format!("{api_url}/api/issues/{issue_id}");
    let output = std::process::Command::new("curl")
        .args(["-s", &url])
        .output()
        .ok()?;

    serde_json::from_slice(&output.stdout).ok()
}

fn get_result(api_url: &str, issue_id: &str) -> String {
    let url = format!("{api_url}/api/issues/{issue_id}/comments");
    let output = std::process::Command::new("curl")
        .args(["-s", &url])
        .output()
        .ok();
    match output {
        Some(out) => {
            let comments: Vec<serde_json::Value> =
                serde_json::from_slice(&out.stdout).unwrap_or_default();
            comments
                .iter()
                .rev()
                .find(|c| c.get("tag").and_then(|t| t.as_str()) == Some("result"))
                .and_then(|c| c.get("body").and_then(|b| b.as_str()))
                .unwrap_or_default()
                .to_string()
        }
        None => String::new(),
    }
}

fn wait_for_terminal(api_url: &str, issue_id: &str) {
    for _ in 0..120 {
        if let Some(json) = get_issue_json(api_url, issue_id) {
            if let Some(status) = json.get("status").and_then(|v| v.as_str()) {
                match status {
                    "completed" | "failed" | "validation_failed" | "blocked" => return,
                    _ => {}
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    eprintln!("Timed out waiting for issue {issue_id}");
}
