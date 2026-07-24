//! Provider-neutral inspection of Loom's trusted MCP registry.

use axum::{extract::State, Json};
use weaver_api::McpRegistryView;

use super::{ApiResult, AppState};

/// Lists compiled-in adapters and their versioned capability sets.  Commands
/// are intentionally absent: profiles select a reviewed capability name, never
/// executable process configuration.
pub(super) async fn list_mcps(State(_st): State<AppState>) -> ApiResult<Json<McpRegistryView>> {
    Ok(Json(crate::mcp::registry()))
}
