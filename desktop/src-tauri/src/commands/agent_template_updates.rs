use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::managed_agents::update_transaction::{
    ImmutableUpdateTarget, JournalHandoffIdentity, JournalRuntimeGeneration, SelectedAgentRecords,
    UpdateTransaction, UpdateTransactionJournal, UpdateTransactionStage,
};
use crate::{
    app_state::AppState,
    managed_agents::{
        agent_readiness, claim_managed_agent_runtime_pairs,
        clear_managed_agent_runtime_pair_claims, drain_managed_agent_pair_for_update,
        find_managed_agent_mut, known_acp_runtime, load_global_agent_config, load_managed_agents,
        load_personas, managed_agent_runtime_keys, resolve_effective_agent_env,
        save_managed_agents_for_operation, start_managed_agent_runtime_pair_authorized,
        start_managed_agent_runtime_pair_authorized_with_nonce,
        start_managed_agent_runtime_pair_lazy, terminate_untracked_pair_runtime, AgentReadiness,
        AgentTemplateVersionRef, BackendKind, ManagedAgentRecord, ManagedAgentRuntimeKey,
        ManagedAgentRuntimeLifecycle, ManagedAgentUpdateDrainError, PlannedUpdateIdentity,
    },
};

use super::agent_template_versions::load_agent_template_version;

const UPDATE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_READY_POLL: Duration = Duration::from_millis(200);
const AGENT_TEMPLATE_UPDATE_PROGRESS_EVENT: &str = "agent-template-update-progress";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAgentTemplateUpdateRequest {
    pub request_id: String,
    pub persona_id: String,
    pub expected_version: AgentTemplateVersionRef,
    pub selected_pubkeys: Vec<String>,
    #[serde(default)]
    pub connection_bindings_by_pubkey: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTemplateUpdateProgressStage {
    PreparingUpdate,
    FinishingCurrentTask,
    StartingUpdatedAgent,
    CheckingUpdate,
    Updated,
    UpdateRolledBack,
    NeedsAttention,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdateProgress {
    pub request_id: String,
    pub stage: AgentTemplateUpdateProgressStage,
}

fn validate_update_request_id(request_id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(request_id)
        .map(|_| ())
        .map_err(|_| "The agent update request id is invalid.".to_string())
}

fn emit_update_progress(
    app: &AppHandle,
    request_id: &str,
    stage: AgentTemplateUpdateProgressStage,
) {
    if let Err(error) = app.emit(
        AGENT_TEMPLATE_UPDATE_PROGRESS_EVENT,
        AgentTemplateUpdateProgress {
            request_id: request_id.to_string(),
            stage,
        },
    ) {
        eprintln!("buzz-desktop: failed to emit agent template update progress: {error}");
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdatePreview {
    pub persona_id: String,
    pub persona_name: String,
    pub target_version: AgentTemplateVersionRef,
    pub target_version_token: String,
    pub target_tool_requirements: Vec<crate::managed_agents::AgentToolRequirement>,
    pub target_skills: Vec<crate::managed_agents::AgentSkill>,
    pub agents: Vec<AgentTemplateUpdateTarget>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateToolRequirementChange {
    pub before: crate::managed_agents::AgentToolRequirement,
    pub after: crate::managed_agents::AgentToolRequirement,
    /// Whether the human-readable Tool label changed.
    pub label_changed: bool,
    /// Whether the stable Tool capability identifier changed.
    pub capability_changed: bool,
    /// Whether the Tool became required or optional.
    pub required_changed: bool,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateToolChanges {
    pub added: Vec<crate::managed_agents::AgentToolRequirement>,
    pub changed: Vec<AgentTemplateToolRequirementChange>,
    pub removed: Vec<crate::managed_agents::AgentToolRequirement>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateSkillChange {
    pub before: crate::managed_agents::AgentSkill,
    pub after: crate::managed_agents::AgentSkill,
    /// Exact file additions, edits, and removals within this Skill.
    pub file_changes: AgentTemplateSkillFileChanges,
}

/// One exact portable Skill file, safe for byte-for-byte review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateSkillFileContent {
    pub path: String,
    pub content: String,
}

/// An exact before/after content change for one stable Skill file path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateSkillFileContentChange {
    pub path: String,
    pub before: String,
    pub after: String,
}

/// Exact file-level changes within one changed Skill.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateSkillFileChanges {
    pub added: Vec<AgentTemplateSkillFileContent>,
    pub changed: Vec<AgentTemplateSkillFileContentChange>,
    pub removed: Vec<AgentTemplateSkillFileContent>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateSkillChanges {
    pub added: Vec<crate::managed_agents::AgentSkill>,
    pub changed: Vec<AgentTemplateSkillChange>,
    pub removed: Vec<crate::managed_agents::AgentSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateInstructionChange {
    pub before: String,
    pub after: String,
    pub private_override_preserved: bool,
}

/// A before/after change for a nullable template string setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateOptionalStringChange {
    pub before: Option<String>,
    pub after: Option<String>,
}

/// A before/after change for template parallelism.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateParallelismChange {
    pub before: u32,
    pub after: u32,
}

/// Changes to who may instruct an existing agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateAccessChange {
    pub before: crate::managed_agents::RespondTo,
    pub after: crate::managed_agents::RespondTo,
    /// Public keys added to the template allowlist.
    pub allowlist_added: Vec<String>,
    /// Public keys removed from the template allowlist.
    pub allowlist_removed: Vec<String>,
}

/// Redacted local environment changes. Values are intentionally unrepresentable.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateEnvironmentChanges {
    pub added_keys: Vec<String>,
    pub changed_keys: Vec<String>,
    pub removed_keys: Vec<String>,
}

/// Existing-agent-relevant settings changed by one template version.
///
/// Template identity and name pools are intentionally absent because updating
/// an existing agent does not apply them.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateVersionChanges {
    pub runtime: Option<AgentTemplateOptionalStringChange>,
    pub provider: Option<AgentTemplateOptionalStringChange>,
    pub model: Option<AgentTemplateOptionalStringChange>,
    pub access: Option<AgentTemplateAccessChange>,
    pub parallelism: Option<AgentTemplateParallelismChange>,
    pub environment: AgentTemplateEnvironmentChanges,
}

/// Private settings that continue to win after the template pin advances.
///
/// Only overrides relevant to this version's changes are disclosed. Secret
/// values and unrelated local environment keys are never serialized.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateOverridesPreserved {
    pub instructions: bool,
    pub runtime: bool,
    pub model: bool,
    pub provider: bool,
    pub skills: bool,
    pub local_environment: bool,
    pub local_environment_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdateTarget {
    pub pubkey: String,
    pub name: String,
    pub current_version: Option<String>,
    pub target_version: AgentTemplateVersionRef,
    pub running_relays: Vec<String>,
    pub eligible: bool,
    pub blocked_reason: Option<String>,
    pub project_scope: Option<crate::managed_agents::AgentProjectScope>,
    pub connection_bindings: BTreeMap<String, String>,
    pub instruction_change: Option<AgentTemplateInstructionChange>,
    pub tool_changes: AgentTemplateToolChanges,
    pub skill_changes: AgentTemplateSkillChanges,
    pub version_changes: AgentTemplateVersionChanges,
    pub overrides_preserved: AgentTemplateOverridesPreserved,
    pub tool_binding_issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTemplateUpdateOutcome {
    Updated,
    UpdatedStopped,
    RolledBack,
    RollbackFailed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdateResult {
    pub pubkey: String,
    pub name: String,
    pub outcome: AgentTemplateUpdateOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAgentTemplateUpdateResponse {
    pub persona_id: String,
    pub version: AgentTemplateVersionRef,
    pub rolled_back: bool,
    pub agents: Vec<AgentTemplateUpdateResult>,
}

mod apply;
mod recovery;
mod support;
pub use recovery::RestoreInterruptedAgentTemplateUpdateRequest;
pub use support::preview_agent_template_update;
use support::*;

pub(crate) use recovery::restart_original_pairs;

#[cfg(test)]
use recovery::{
    classify_drain_failure, failure_response, restore_config_preserving_runtime_state,
    rollback_guard_allows, same_nonvolatile_config, DrainFailureState,
};

/// Return only sanitized interrupted-update metadata for the recovery UI.
#[tauri::command]
pub fn list_agent_template_update_recoveries(
    app: AppHandle,
) -> Result<Vec<crate::managed_agents::update_transaction::AgentTemplateUpdateRecoveryStatus>, String>
{
    recovery::list_agent_template_update_recoveries(app)
}

/// Explicitly restore the previous agent versions after an interrupted update.
#[tauri::command]
pub async fn restore_interrupted_agent_template_update(
    input: RestoreInterruptedAgentTemplateUpdateRequest,
    app: AppHandle,
) -> Result<(), String> {
    recovery::restore_interrupted_agent_template_update(input, app).await
}

/// Apply one published template version to the selected linked agents.
#[tauri::command]
pub async fn apply_agent_template_update(
    input: ApplyAgentTemplateUpdateRequest,
    app: AppHandle,
) -> Result<ApplyAgentTemplateUpdateResponse, String> {
    apply::apply_agent_template_update(input, app).await
}

#[cfg(test)]
#[path = "agent_template_updates/tests.rs"]
mod tests;
