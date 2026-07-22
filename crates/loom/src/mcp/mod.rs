//! Registry for Loom's built-in, restricted MCP adapters.
//!
//! Profiles select reviewed capability sets such as `mcp/github/comment`.
//! Loom expands those names into exact Claude permission rules when it stamps a
//! session, then derives the required adapter processes from that immutable
//! policy. Repositories can choose sets, but cannot inject executable adapter
//! configuration.

use anyhow::{bail, Result};
use serde_json::{Map, Value};
use std::{future::Future, pin::Pin};

pub(crate) mod github;

type ServeFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

struct Adapter {
    name: &'static str,
    server_name: &'static str,
    expand_tool_set: fn(&str) -> Option<Vec<String>>,
    is_permission_rule: fn(&str) -> bool,
    server_config: fn() -> Value,
    serve: fn() -> ServeFuture,
}

const ADAPTERS: &[Adapter] = &[github::ADAPTER];

pub(crate) fn is_tool_set(name: &str) -> bool {
    ADAPTERS
        .iter()
        .any(|adapter| (adapter.expand_tool_set)(name).is_some())
}

/// Expand profile-facing capability sets into the exact rules persisted on a
/// session. Ordinary Claude rules are retained and duplicates are removed
/// without changing their order.
pub(crate) fn expand_tool_sets(rules: &[String]) -> Result<Vec<String>> {
    let mut expanded = Vec::new();
    for rule in rules {
        let tool_rules = ADAPTERS
            .iter()
            .find_map(|adapter| (adapter.expand_tool_set)(rule));
        if let Some(tool_rules) = tool_rules {
            for tool_rule in tool_rules {
                push_unique(&mut expanded, tool_rule);
            }
        } else if rule.starts_with("mcp/") {
            bail!("unknown built-in MCP tool set '{rule}'");
        } else {
            push_unique(&mut expanded, rule.clone());
        }
    }
    Ok(expanded)
}

/// Build only the MCP server definitions needed by the session's exact
/// permission rules. Adapter commands come from this trusted registry, never
/// from repository-controlled profile data.
pub(crate) fn server_configs(allowed_rules: &[String]) -> Map<String, Value> {
    let mut servers = Map::new();
    for adapter in ADAPTERS {
        if allowed_rules
            .iter()
            .any(|rule| (adapter.is_permission_rule)(rule))
        {
            servers.insert(adapter.server_name.to_string(), (adapter.server_config)());
        }
    }
    servers
}

pub async fn serve(adapter: &str) -> Result<()> {
    let adapter = ADAPTERS
        .iter()
        .find(|candidate| candidate.name == adapter)
        .ok_or_else(|| anyhow::anyhow!("unknown built-in MCP adapter '{adapter}'"))?;
    (adapter.serve)().await
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::{expand_tool_sets, server_configs};

    #[test]
    fn expands_sets_and_preserves_ordinary_rules() {
        let rules = vec![
            "Read(./**)".to_string(),
            "mcp/github/comment".to_string(),
            "Read(./**)".to_string(),
        ];
        let expanded = expand_tool_sets(&rules).unwrap();
        assert_eq!(expanded[0], "Read(./**)");
        assert_eq!(expanded.len(), 7);
        assert!(expanded.contains(&"mcp__loom_github__issue_edit".to_string()));
    }

    #[test]
    fn rejects_unknown_namespaced_sets() {
        let error = expand_tool_sets(&["mcp/github/admin".to_string()]).unwrap_err();
        assert!(error.to_string().contains("unknown built-in MCP tool set"));
    }

    #[test]
    fn selects_servers_from_exact_session_permissions() {
        assert!(server_configs(&["Read(./**)".to_string()]).is_empty());
        let servers = server_configs(&["mcp__loom_github__issue_view".to_string()]);
        assert_eq!(servers.len(), 1);
        assert!(servers.contains_key("loom_github"));
    }
}
