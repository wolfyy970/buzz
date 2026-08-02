use serde::{Deserialize, Serialize};

/// One portable tool capability declared by an agent template.
///
/// The requirement names what the agent needs, not how it is connected.
/// Project-owned connection records bind the stable `id` to concrete MCP
/// transport and local credentials when an agent is launched.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentToolRequirement {
    /// Stable template-local identifier used by agent connection bindings.
    pub id: String,
    /// User-facing name, for example "Analytics reports".
    pub label: String,
    /// Stable capability identifier, for example "analytics.reports.read".
    pub capability: String,
    /// Whether the agent may continue without this tool.
    #[serde(default = "default_tool_requirement_required")]
    pub required: bool,
}

fn default_tool_requirement_required() -> bool {
    true
}

/// Stable Project boundary for a managed agent and its tool connections.
///
/// `repo_address` is the canonical NIP-34 coordinate (`30617:<owner>:<d-tag>`),
/// not the desktop's local Project id. `channel_id` is the Project discussion
/// channel and is enforced through `BUZZ_ACP_CHANNELS` whenever Project tools
/// are present, so those tools cannot be used from the agent's other channels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct AgentProjectScope {
    /// Canonical relay URL for the active Buzz community.
    pub relay_url: String,
    /// Identity that owns the local connection records and credentials.
    pub operator_pubkey: String,
    pub repo_address: String,
    pub channel_id: String,
}
