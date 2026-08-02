//! Private persona revision history persisted with managed-agent instances.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::RespondTo;

/// One prior, locally pinned persona revision for a managed-agent instance.
///
/// This is rollback material, not a public persona projection. It can contain
/// credential-bearing persona env values, so it stays inside the restricted
/// local managed-agent store and is deliberately absent from kind:30177 and
/// portable snapshot projections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaSnapshotHistoryEntry {
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub source_version: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pinned_persona_env_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub respond_to: RespondTo,
    #[serde(default)]
    pub respond_to_allowlist: Vec<String>,
    #[serde(default = "super::default_agent_parallelism")]
    pub parallelism: u32,
}
