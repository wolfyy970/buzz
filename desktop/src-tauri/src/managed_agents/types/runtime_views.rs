use serde::{Deserialize, Serialize};

use super::RespondTo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayAgentInfo {
    pub pubkey: String,
    pub name: String,
    pub agent_type: String,
    pub channels: Vec<String>,
    #[serde(default)]
    pub channel_ids: Vec<String>,
    pub capabilities: Vec<String>,
    pub status: String,
    #[serde(default)]
    pub respond_to: Option<RespondTo>,
    #[serde(default)]
    pub respond_to_allowlist: Vec<String>,
}

/// Typed relay-mesh configuration carried on a
/// [`ManagedAgentRecord`](super::ManagedAgentRecord).
///
/// Feature-independent on purpose: the field is always present in the record
/// schema so saved agents round-trip identically with or without mesh support.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayMeshConfig {
    /// The served model id this agent routes to (for example, "Qwen3").
    ///
    /// The alias accepts TypeScript's camelCase request while serialization
    /// remains snake_case for stable saved records.
    #[serde(alias = "modelRef")]
    pub model_ref: String,
}
