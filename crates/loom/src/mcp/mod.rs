//! Registry for Loom's built-in, restricted MCP adapters.
//!
//! Profiles select reviewed capability sets such as `mcp/github/comment`.
//! Loom expands those names into exact Claude permission rules when it stamps a
//! session, then derives the required adapter processes from that immutable
//! policy. Repositories can choose sets, but cannot inject executable adapter
//! configuration.

use anyhow::{bail, Result};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin};
use weaver_api::{McpAdapterView, McpCapabilitySetView, McpRegistryView};

pub(crate) mod github;

type ServeFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

pub(crate) struct Adapter {
    name: &'static str,
    server_name: &'static str,
    description: &'static str,
    capability_sets: fn() -> &'static [CapabilitySet],
    expand_tool_set: fn(&str) -> Option<Vec<String>>,
    is_permission_rule: fn(&str) -> bool,
    server_config: fn() -> Value,
    serve: fn() -> ServeFuture,
}

/// A stable, provider-neutral set of MCP operations.  A set's digest is part
/// of the operator-visible contract: adding a tool requires a new versioned
/// identity rather than silently widening an unchanged profile selection.
pub(crate) struct CapabilitySet {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub tools: &'static [&'static str],
}

const ADAPTERS: &[Adapter] = &[github::ADAPTER];

pub(crate) fn is_tool_set(name: &str) -> bool {
    ADAPTERS
        .iter()
        .any(|adapter| (adapter.expand_tool_set)(name).is_some())
}

pub(crate) fn registry() -> McpRegistryView {
    let mut adapters = Vec::new();
    let mut capability_sets = Vec::new();
    for adapter in ADAPTERS {
        adapters.push(McpAdapterView {
            name: adapter.name.to_string(),
            description: adapter.description.to_string(),
            server_name: adapter.server_name.to_string(),
        });
        for set in (adapter.capability_sets)() {
            let tools = set.tools.iter().map(|tool| tool.to_string()).collect();
            capability_sets.push(McpCapabilitySetView {
                name: set.name.to_string(),
                version: set.version.to_string(),
                digest: capability_set_digest(set),
                description: set.description.to_string(),
                adapter: adapter.name.to_string(),
                tools,
            });
        }
    }
    McpRegistryView {
        adapters,
        capability_sets,
    }
}

fn capability_set_digest(set: &CapabilitySet) -> String {
    let mut hasher = Sha256::new();
    hasher.update(set.name);
    hasher.update([0]);
    hasher.update(set.version);
    for tool in set.tools {
        hasher.update([0]);
        hasher.update(tool);
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
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
            "mcp/github/comment@v1".to_string(),
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

    #[test]
    fn registry_exposes_versioned_provider_neutral_sets() {
        let registry = super::registry();
        let set = &registry.capability_sets[0];
        assert_eq!(set.name, "mcp/github/comment@v1");
        assert!(set.digest.starts_with("sha256:"));
        assert_eq!(set.tools.len(), 6);
    }
}
