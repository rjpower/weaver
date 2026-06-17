//! Claude Code raw transcript → [`iris`](super::iris) format.
//!
//! Claude Code writes one JSON object per line. The conversation lives in
//! `user` / `assistant` records, each with a `message.content` that is either a
//! plain string or an array of typed blocks (`text`, `thinking`, `tool_use`,
//! `tool_result`). Tool results come back on a *following* `user` record whose
//! content is only `tool_result` blocks — we fold those into an assistant-role
//! message so they render under the tool call that produced them. Bookkeeping
//! records (mode, permission-mode, snapshots) are dropped.

use serde_json::Value;

use super::iris::{Block, Log, Message, Role};

/// Convert a Claude Code transcript (concatenated JSONL is fine) into an iris
/// [`Log`]. Malformed lines and non-message records are skipped.
pub fn to_iris(jsonl: &str) -> Log {
    let records: Vec<Value> = jsonl
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();

    let session_id = records
        .iter()
        .find_map(|r| r.get("sessionId").and_then(Value::as_str))
        .map(str::to_string);
    let model = records
        .iter()
        .find_map(|r| {
            r.get("message")
                .and_then(|m| m.get("model"))
                .and_then(Value::as_str)
        })
        .map(str::to_string);
    let cwd = records
        .iter()
        .find_map(|r| r.get("cwd").and_then(Value::as_str))
        .map(str::to_string);

    let mut messages = Vec::new();
    for rec in &records {
        if let Some(msg) = convert_record(rec) {
            messages.push(msg);
        }
    }

    Log {
        source: "claude".to_string(),
        session_id,
        model,
        cwd,
        messages,
    }
}

fn convert_record(rec: &Value) -> Option<Message> {
    let kind = rec.get("type").and_then(Value::as_str)?;
    if kind != "user" && kind != "assistant" {
        return None;
    }
    let timestamp = rec
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_string);
    let is_meta = rec.get("isMeta").and_then(Value::as_bool) == Some(true);
    let content = rec.get("message").and_then(|m| m.get("content"));

    // Plain-string content: a real human prompt or a bare assistant reply.
    if let Some(text) = content.and_then(Value::as_str) {
        if text.trim().is_empty() {
            return None;
        }
        let role = role_for(kind, is_meta);
        return Some(Message::new(role, timestamp, vec![Block::text(text)]));
    }

    let blocks = content.and_then(Value::as_array)?;
    let converted = convert_blocks(blocks);
    if converted.is_empty() {
        return None;
    }

    // A `user` record carrying only tool_result blocks is the result-bearer for
    // the preceding assistant tool calls — attribute it to the assistant so it
    // groups under that turn. One that also carries text is a real prompt.
    let only_results = kind == "user"
        && converted
            .iter()
            .all(|b| matches!(b, Block::ToolResult { .. } | Block::Image));
    let role = if only_results {
        Role::Assistant
    } else {
        role_for(kind, is_meta)
    };
    Some(Message::new(role, timestamp, converted))
}

fn role_for(kind: &str, is_meta: bool) -> Role {
    if is_meta {
        Role::Context
    } else if kind == "assistant" {
        Role::Assistant
    } else {
        Role::User
    }
}

fn convert_blocks(blocks: &[Value]) -> Vec<Block> {
    let mut out = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    out.push(Block::text(t));
                }
            }
            Some("thinking") => {
                if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                    out.push(Block::thinking(t));
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                out.push(Block::ToolUse { name, input });
            }
            Some("tool_result") => {
                let output = content_to_text(block.get("content"));
                let is_error = block.get("is_error").and_then(Value::as_bool) == Some(true);
                out.push(Block::tool_result(output, is_error));
            }
            Some("image") => out.push(Block::Image),
            _ => {}
        }
    }
    out
}

/// Flatten a content value to plain text: a bare string passes through; an array
/// of `{type:text,text}` blocks is concatenated.
fn content_to_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(v: Value) -> String {
        v.to_string()
    }

    #[test]
    fn converts_a_turn_with_thinking_tool_call_and_result() {
        let jsonl = [
            line(json!({"type": "mode", "sessionId": "s1"})),
            line(json!({
                "type": "user", "sessionId": "s1", "cwd": "/w",
                "timestamp": "2026-06-17T10:00:00.000Z",
                "message": {"role": "user", "content": "Fix the bug"}
            })),
            line(json!({
                "type": "assistant", "sessionId": "s1",
                "timestamp": "2026-06-17T10:00:01.000Z",
                "message": {"role": "assistant", "model": "claude-opus-4-8", "content": [
                    {"type": "thinking", "thinking": "Let me look."},
                    {"type": "text", "text": "On it."},
                    {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}
                ]}
            })),
            line(json!({
                "type": "user", "sessionId": "s1",
                "timestamp": "2026-06-17T10:00:02.000Z",
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "content": "total 0", "is_error": false}
                ]}
            })),
        ]
        .join("\n");

        let log = to_iris(&jsonl);
        assert_eq!(log.source, "claude");
        assert_eq!(log.session_id.as_deref(), Some("s1"));
        assert_eq!(log.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(log.cwd.as_deref(), Some("/w"));
        // mode record dropped → 3 messages (user, assistant, tool-result).
        assert_eq!(log.messages.len(), 3);
        assert!(matches!(log.messages[0].role, Role::User));
        assert!(matches!(log.messages[1].role, Role::Assistant));
        // The tool-result-only user record is folded onto the assistant.
        assert!(matches!(log.messages[2].role, Role::Assistant));
        assert!(matches!(
            log.messages[2].blocks[0],
            Block::ToolResult { .. }
        ));
    }

    #[test]
    fn marks_meta_records_as_context() {
        let jsonl = line(json!({
            "type": "user", "isMeta": true,
            "message": {"role": "user", "content": "injected primer"}
        }));
        let log = to_iris(&jsonl);
        assert_eq!(log.messages.len(), 1);
        assert!(matches!(log.messages[0].role, Role::Context));
    }
}
