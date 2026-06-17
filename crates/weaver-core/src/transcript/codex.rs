//! Codex raw rollout transcript → [`iris`](super::iris) format.
//!
//! Codex writes a `rollout-*.jsonl` per session, one JSON object per line, each
//! `{ timestamp, type, payload }`. Two streams are interleaved: `response_item`
//! (the model's Responses-API items — messages, reasoning, function/tool calls
//! and their outputs) and `event_msg` (UI-level events). We take the backbone
//! from `response_item` and the *clean* user turns from `event_msg/user_message`
//! — the `response_item` user messages are wrapped in injected context
//! (AGENTS.md, environment, permissions), so using the event stream for prompts
//! keeps the log clean. Everything else (`token_count`, `task_*`, duplicate
//! `agent_message`, …) is dropped.
//!
//! Codex reasoning is usually `encrypted_content` with no plaintext summary, so
//! thinking only appears when a summary is present.

use serde_json::Value;

use super::iris::{Block, Log, Message, Role};

/// Convert a Codex rollout transcript (concatenated JSONL is fine) into an iris
/// [`Log`]. Malformed lines and non-conversational records are skipped.
pub fn to_iris(jsonl: &str) -> Log {
    let records: Vec<Value> = jsonl
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();

    let mut session_id = None;
    let mut model = None;
    let mut cwd = None;
    let mut messages = Vec::new();

    for rec in &records {
        let payload = rec.get("payload");
        let top_type = rec.get("type").and_then(Value::as_str).unwrap_or("");
        let ts = rec
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string);

        match top_type {
            "session_meta" => {
                if let Some(p) = payload {
                    session_id = session_id
                        .or_else(|| p.get("id").and_then(Value::as_str).map(str::to_string));
                    cwd = cwd.or_else(|| p.get("cwd").and_then(Value::as_str).map(str::to_string));
                }
            }
            "turn_context" => {
                if let Some(p) = payload {
                    model = model
                        .or_else(|| p.get("model").and_then(Value::as_str).map(str::to_string));
                    cwd = cwd.or_else(|| p.get("cwd").and_then(Value::as_str).map(str::to_string));
                }
            }
            "event_msg" => {
                if let Some(msg) = convert_event_msg(payload, ts) {
                    messages.push(msg);
                }
            }
            "response_item" => {
                if let Some(msg) = convert_response_item(payload, ts) {
                    messages.push(msg);
                }
            }
            _ => {}
        }
    }

    Log {
        source: "codex".to_string(),
        session_id,
        model,
        cwd,
        messages,
    }
}

/// Only `user_message` is taken from the event stream — the clean text the human
/// actually typed. Everything else there duplicates `response_item` or is noise.
fn convert_event_msg(payload: Option<&Value>, ts: Option<String>) -> Option<Message> {
    let p = payload?;
    if p.get("type").and_then(Value::as_str)? != "user_message" {
        return None;
    }
    let text = p.get("message").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }
    Some(Message::new(Role::User, ts, vec![Block::text(text)]))
}

fn convert_response_item(payload: Option<&Value>, ts: Option<String>) -> Option<Message> {
    let p = payload?;
    match p.get("type").and_then(Value::as_str)? {
        "message" => convert_message(p, ts),
        "reasoning" => convert_reasoning(p, ts),
        "function_call" => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("tool");
            // `arguments` is a JSON string; surface it as structured input when it
            // parses, else keep the raw string.
            let input = p
                .get("arguments")
                .and_then(Value::as_str)
                .map(parse_json_or_string)
                .unwrap_or(Value::Null);
            Some(Message::new(
                Role::Assistant,
                ts,
                vec![Block::ToolUse {
                    name: name.to_string(),
                    input,
                }],
            ))
        }
        "custom_tool_call" => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("tool");
            // `input` here is a raw string (e.g. an apply_patch body).
            let input = p.get("input").cloned().unwrap_or(Value::Null);
            Some(Message::new(
                Role::Assistant,
                ts,
                vec![Block::ToolUse {
                    name: name.to_string(),
                    input,
                }],
            ))
        }
        "function_call_output" | "custom_tool_call_output" => {
            let (output, is_error) = tool_output(p.get("output"));
            if output.trim().is_empty() {
                return None;
            }
            Some(Message::new(
                Role::Assistant,
                ts,
                vec![Block::tool_result(output, is_error)],
            ))
        }
        "web_search_call" => {
            let input = p
                .get("action")
                .or_else(|| p.get("query"))
                .cloned()
                .unwrap_or(Value::Null);
            Some(Message::new(
                Role::Assistant,
                ts,
                vec![Block::ToolUse {
                    name: "web_search".to_string(),
                    input,
                }],
            ))
        }
        _ => None,
    }
}

/// A `response_item` message. `developer` is system/permissions material →
/// [`Role::Context`]; `assistant` text is a reply; `user` is dropped (the clean
/// prompt comes from `event_msg/user_message`).
fn convert_message(p: &Value, ts: Option<String>) -> Option<Message> {
    let role = p.get("role").and_then(Value::as_str)?;
    let text = text_from_content(p.get("content"));
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    match role {
        "assistant" => Some(Message::new(Role::Assistant, ts, vec![Block::text(text)])),
        "developer" => Some(Message::new(Role::Context, ts, vec![Block::text(text)])),
        _ => None,
    }
}

/// Reasoning, only when a plaintext summary/content is present (it is usually
/// just `encrypted_content`).
fn convert_reasoning(p: &Value, ts: Option<String>) -> Option<Message> {
    let mut text = text_from_content(p.get("summary"));
    if text.trim().is_empty() {
        text = text_from_content(p.get("content"));
    }
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(Message::new(
        Role::Assistant,
        ts,
        vec![Block::thinking(text)],
    ))
}

/// Pull display text out of a Responses-API content value: a bare string, or an
/// array of `{text}` / `{type, text}` blocks (`input_text`, `output_text`,
/// `summary_text`).
fn text_from_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| match b {
                Value::String(s) => Some(s.clone()),
                Value::Object(_) => b.get("text").and_then(Value::as_str).map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A tool output, with Codex's two output envelopes unwrapped to just the text
/// (and whether the command failed): the JSON `{"output": "...", "metadata": {
/// "exit_code": N }}` form (apply_patch, MCP) and the `exec_command` text form
/// (a `Chunk ID:` / `Process exited with code N` / `Output:` preamble). Anything
/// else passes through unchanged with no error flag.
fn tool_output(output: Option<&Value>) -> (String, bool) {
    let Some(v) = output else {
        return (String::new(), false);
    };
    let raw = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&raw) {
        if let Some(inner) = obj.get("output").and_then(Value::as_str) {
            let failed = obj
                .get("metadata")
                .and_then(|m| m.get("exit_code"))
                .and_then(Value::as_i64)
                .map(|c| c != 0)
                .unwrap_or(false);
            return (inner.to_string(), failed);
        }
    }
    if let Some(unwrapped) = strip_exec_envelope(&raw) {
        return unwrapped;
    }
    (raw, false)
}

/// Strip the `exec_command` text preamble, returning just the command's output
/// and whether it exited non-zero. The preamble is bookkeeping lines
/// (`Chunk ID:`, `Wall time:`, `Process exited with code N`, `Original token
/// count:`) terminated by an `Output:` line; the real output follows. `None`
/// when the text isn't in that shape, so non-exec outputs are left untouched.
fn strip_exec_envelope(raw: &str) -> Option<(String, bool)> {
    if !raw.starts_with("Chunk ID:") && !raw.contains("\nProcess exited with code ") {
        return None;
    }
    let (preamble, body) = raw.split_once("\nOutput:\n").or_else(|| {
        // An empty-output command ends at a bare trailing `Output:`.
        raw.split_once("\nOutput:").map(|(p, _)| (p, ""))
    })?;
    let failed = preamble
        .lines()
        .find_map(|l| l.strip_prefix("Process exited with code "))
        .and_then(|c| c.trim().parse::<i64>().ok())
        .map(|c| c != 0)
        .unwrap_or(false);
    Some((body.to_string(), failed))
}

/// Parse a JSON string into a [`Value`], or wrap it as a string when it isn't
/// valid JSON.
fn parse_json_or_string(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(v: Value) -> String {
        v.to_string()
    }

    #[test]
    fn converts_meta_user_assistant_tools_and_drops_noise() {
        let jsonl = [
            line(json!({"timestamp": "t0", "type": "session_meta",
                "payload": {"id": "sess-1", "cwd": "/home/me/proj"}})),
            line(json!({"timestamp": "t0", "type": "turn_context",
                "payload": {"model": "gpt-5.5", "cwd": "/home/me/proj"}})),
            // Clean user prompt from the event stream.
            line(json!({"timestamp": "t1", "type": "event_msg",
                "payload": {"type": "user_message", "message": "review this"}})),
            // Noise that must be dropped.
            line(json!({"timestamp": "t1", "type": "event_msg",
                "payload": {"type": "token_count", "info": {}}})),
            line(json!({"timestamp": "t1", "type": "event_msg",
                "payload": {"type": "agent_message", "message": "dup"}})),
            // The response_item user message (wrapped context) must be dropped.
            line(json!({"timestamp": "t1", "type": "response_item",
                "payload": {"type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "# AGENTS.md ..."}]}})),
            // Developer message → context.
            line(json!({"timestamp": "t1", "type": "response_item",
                "payload": {"type": "message", "role": "developer",
                    "content": [{"type": "input_text", "text": "permissions ..."}]}})),
            // Assistant reply.
            line(json!({"timestamp": "t2", "type": "response_item",
                "payload": {"type": "message", "role": "assistant",
                    "content": [{"type": "output_text", "text": "Looking now."}]}})),
            // A function call + its output (with the JSON envelope + nonzero exit).
            line(json!({"timestamp": "t3", "type": "response_item",
                "payload": {"type": "function_call", "name": "exec_command",
                    "arguments": "{\"cmd\":\"ls\"}", "call_id": "c1"}})),
            line(json!({"timestamp": "t4", "type": "response_item",
                "payload": {"type": "function_call_output", "call_id": "c1",
                    "output": "{\"output\":\"boom\",\"metadata\":{\"exit_code\":1}}"}})),
            // A custom tool call (raw-string input).
            line(json!({"timestamp": "t5", "type": "response_item",
                "payload": {"type": "custom_tool_call", "name": "apply_patch",
                    "input": "*** Begin Patch", "call_id": "c2"}})),
        ]
        .join("\n");

        let log = to_iris(&jsonl);
        assert_eq!(log.source, "codex");
        assert_eq!(log.session_id.as_deref(), Some("sess-1"));
        assert_eq!(log.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(log.cwd.as_deref(), Some("/home/me/proj"));

        let roles: Vec<Role> = log.messages.iter().map(|m| m.role).collect();
        // user, developer→context, assistant, tool-call, tool-output, custom-call.
        // The agent_message dup and the response_item user message are gone.
        assert_eq!(roles.len(), 6, "messages: {:#?}", log.messages);
        assert!(matches!(roles[0], Role::User));
        assert!(matches!(roles[1], Role::Context));
        assert!(matches!(roles[2], Role::Assistant));

        // The user prompt is the clean event-stream text, not the AGENTS wrapper.
        match &log.messages[0].blocks[0] {
            Block::Text { text } => assert_eq!(text, "review this"),
            b => panic!("expected user text, got {b:?}"),
        }
        // exec_command arguments parsed into structured input.
        match &log.messages[3].blocks[0] {
            Block::ToolUse { name, input } => {
                assert_eq!(name, "exec_command");
                assert_eq!(input.get("cmd").and_then(Value::as_str), Some("ls"));
            }
            b => panic!("expected tool_use, got {b:?}"),
        }
        // Output envelope unwrapped + flagged as an error (exit_code 1).
        match &log.messages[4].blocks[0] {
            Block::ToolResult { output, is_error } => {
                assert_eq!(output, "boom");
                assert!(is_error);
            }
            b => panic!("expected tool_result, got {b:?}"),
        }
        // Raw-string custom tool input preserved.
        match &log.messages[5].blocks[0] {
            Block::ToolUse { name, input } => {
                assert_eq!(name, "apply_patch");
                assert_eq!(input.as_str(), Some("*** Begin Patch"));
            }
            b => panic!("expected tool_use, got {b:?}"),
        }
    }

    #[test]
    fn strips_the_exec_command_envelope_and_reads_the_exit_code() {
        let raw = "Chunk ID: abc\nWall time: 0.1 seconds\nProcess exited with code 2\n\
                   Original token count: 9\nOutput:\nbad command\nsecond line\n";
        let (body, failed) = strip_exec_envelope(raw).expect("recognized envelope");
        assert_eq!(body, "bad command\nsecond line\n");
        assert!(failed, "nonzero exit → error");

        // A clean exit, output passed through tool_output end-to-end.
        let ok = serde_json::json!("Chunk ID: x\nProcess exited with code 0\nOutput:\nhello\n");
        let (body, failed) = tool_output(Some(&ok));
        assert_eq!(body, "hello\n");
        assert!(!failed);

        // Plain output that isn't an envelope is left untouched.
        assert_eq!(strip_exec_envelope("just some text"), None);
    }
}
