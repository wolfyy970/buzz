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

mod support;
pub use support::preview_agent_template_update;
use support::*;

/// Return only sanitized interrupted-update metadata for the recovery UI.
#[tauri::command]
pub fn list_agent_template_update_recoveries(
    app: AppHandle,
) -> Result<Vec<crate::managed_agents::update_transaction::AgentTemplateUpdateRecoveryStatus>, String>
{
    crate::managed_agents::update_transaction::list_agent_template_update_recoveries(&app)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreInterruptedAgentTemplateUpdateRequest {
    pub transaction_id: String,
    /// Confirms that Buzz may stop every journaled pair before restoring the
    /// previous records. This is mandatory because a candidate may have
    /// accepted work immediately before Desktop crashed.
    pub confirm_stop_buzz_owned_agents: bool,
}

/// Explicit, fail-closed recovery for an interrupted template update.
///
/// Recovery is intentionally available only after relaunch, when no selected
/// runtime is tracked by this Desktop process. The user must confirm that Buzz
/// may stop the exact journaled pairs. The command resumes durable rollback
/// stages and never guesses that an uncertain candidate is safe.
#[tauri::command]
pub async fn restore_interrupted_agent_template_update(
    input: RestoreInterruptedAgentTemplateUpdateRequest,
    app: AppHandle,
) -> Result<(), String> {
    if !input.confirm_stop_buzz_owned_agents {
        return Err(
            "Confirm that Buzz may stop the affected agents before restoring their previous version."
                .to_string(),
        );
    }
    let state = app.state::<AppState>();
    let journal = UpdateTransactionJournal::for_app(&app)?;
    let mut transaction = journal.load(&input.transaction_id)?;
    let selected = transaction.selected.keys().cloned().collect::<Vec<_>>();
    state
        .managed_agent_update_leases
        .ensure_owned(
            &transaction.transaction_id,
            selected.iter().map(String::as_str),
        )
        .map_err(|_| {
            "Restart Buzz before recovering this interrupted update so its durable ownership can be verified."
                .to_string()
        })?;
    {
        let runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;
        if !runtime_keys_for_selected(&runtimes, &selected).is_empty() {
            return Err(
                "Restart Buzz before recovery. An affected agent is still tracked by this Desktop session."
                    .to_string(),
            );
        }
    }
    let original_keys = transaction
        .original_generations
        .iter()
        .map(|generation| generation.key.clone())
        .collect::<Vec<_>>();
    let originals = transaction
        .selected
        .iter()
        .map(|(pubkey, records)| (pubkey.clone(), records.original.clone()))
        .collect::<BTreeMap<_, _>>();
    let attempted = transaction
        .selected
        .iter()
        .map(|(pubkey, records)| (pubkey.clone(), records.attempted.clone()))
        .collect::<BTreeMap<_, _>>();
    let candidates = transaction.candidate_generations.clone();

    match transaction.stage {
        UpdateTransactionStage::Prepared
        | UpdateTransactionStage::UpdateCompleted
        | UpdateTransactionStage::RollbackCompleted => {
            journal.delete(&transaction.transaction_id, transaction.revision)?;
            state.managed_agent_update_leases.clear_recovery(
                &transaction.transaction_id,
                selected.iter().map(String::as_str),
            )?;
            return Ok(());
        }
        UpdateTransactionStage::BeforeCandidateLaunch
        | UpdateTransactionStage::AfterCandidateReady => {
            advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::BeforeCandidateHandoff,
                candidates.clone(),
            )?;
        }
        _ => {}
    }

    for key in &original_keys {
        terminate_untracked_pair_runtime(&app, key).map_err(|error| {
            format!(
                "Buzz could not stop the affected agent on {}. Recovery remains blocked: {error}",
                key.relay_url
            )
        })?;
    }

    if transaction.stage == UpdateTransactionStage::BeforeCandidateHandoff {
        advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::AfterCandidateHandoff,
            candidates.clone(),
        )?;
    }

    match transaction.stage {
        UpdateTransactionStage::BeforeOriginalHandoff
        | UpdateTransactionStage::AfterOriginalHandoff => {
            advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::BeforeOriginalRestart,
                candidates.clone(),
            )?;
        }
        UpdateTransactionStage::BeforeCandidateCommit
        | UpdateTransactionStage::AfterCandidateCommit
        | UpdateTransactionStage::AfterCandidateHandoff => {
            advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::BeforeOriginalRestore,
                candidates.clone(),
            )?;
        }
        _ => {}
    }

    if transaction.stage == UpdateTransactionStage::BeforeOriginalRestore {
        restore_original_records(
            &app,
            &selected,
            &originals,
            &attempted,
            &transaction.transaction_id,
        )?;
        advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::AfterOriginalRestore,
            candidates.clone(),
        )?;
    }
    if transaction.stage == UpdateTransactionStage::AfterOriginalRestore {
        advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::BeforeOriginalRestart,
            candidates.clone(),
        )?;
    }
    if transaction.stage == UpdateTransactionStage::BeforeOriginalRestart {
        let restart_errors =
            restart_runtime_keys(&app, &original_keys, &transaction.transaction_id).await;
        if !restart_errors.is_empty() {
            return Err(format!(
                "The previous version was restored, but one or more agents could not restart: {}",
                restart_errors
                    .into_iter()
                    .map(|(key, error)| format!("{} on {}: {error}", key.pubkey, key.relay_url))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::AfterOriginalRestart,
            candidates.clone(),
        )?;
    }
    if transaction.stage == UpdateTransactionStage::AfterOriginalRestart {
        advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::RollbackCompleted,
            candidates,
        )?;
    }
    if transaction.stage != UpdateTransactionStage::RollbackCompleted {
        return Err(
            "This interrupted update is in a recovery state Buzz cannot resolve automatically."
                .to_string(),
        );
    }
    journal.delete(&transaction.transaction_id, transaction.revision)?;
    state.managed_agent_update_leases.clear_recovery(
        &transaction.transaction_id,
        selected.iter().map(String::as_str),
    )?;
    emit_update_progress(
        &app,
        &transaction.request_id,
        AgentTemplateUpdateProgressStage::UpdateRolledBack,
    );
    Ok(())
}

fn restore_original_records(
    app: &AppHandle,
    selected: &[String],
    originals: &BTreeMap<String, ManagedAgentRecord>,
    attempted: &BTreeMap<String, ManagedAgentRecord>,
    operation_id: &str,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _transition = state
        .managed_agent_runtime_transition
        .lock()
        .map_err(|error| error.to_string())?;
    let _store = state
        .managed_agents_store_lock
        .lock()
        .map_err(|error| error.to_string())?;
    let runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;
    if selected
        .iter()
        .any(|pubkey| !managed_agent_runtime_keys(&runtimes, pubkey).is_empty())
    {
        return Err(
            "An updated agent is still running. Buzz did not restore the previous version because that could start a second writer."
                .to_string(),
        );
    }
    drop(runtimes);
    let mut records = load_managed_agents(app)?;

    // Preflight the whole group before mutating any record. A concurrent
    // instance edit must reject the rollback atomically rather than letting a
    // source-version-only guard overwrite the newer configuration.
    for (pubkey, original) in originals {
        let current = records
            .iter()
            .find(|record| record.pubkey.eq_ignore_ascii_case(pubkey))
            .ok_or_else(|| format!("Agent {pubkey} no longer exists."))?;
        let attempted = attempted
            .get(pubkey)
            .ok_or_else(|| format!("Agent {pubkey} has no attempted update snapshot."))?;
        if !rollback_guard_allows(current, original, attempted) {
            return Err(format!(
                "{} changed while the update was being checked. Its newer configuration was not overwritten.",
                current.name
            ));
        }
    }
    for (pubkey, original) in originals {
        let current = find_managed_agent_mut(&mut records, pubkey)?;
        restore_config_preserving_runtime_state(current, original);
    }
    save_managed_agents_for_operation(app, &records, operation_id)
}

fn normalize_runtime_state(record: &mut ManagedAgentRecord) {
    record.runtime_pid = None;
    record.updated_at.clear();
    record.last_started_at = None;
    record.last_stopped_at = None;
    record.last_exit_code = None;
    record.last_error = None;
    record.last_error_code = None;
}

fn same_nonvolatile_config(current: &ManagedAgentRecord, expected: &ManagedAgentRecord) -> bool {
    let mut current = current.clone();
    let mut expected = expected.clone();
    normalize_runtime_state(&mut current);
    normalize_runtime_state(&mut expected);
    current == expected
}

fn rollback_guard_allows(
    current: &ManagedAgentRecord,
    original: &ManagedAgentRecord,
    attempted: &ManagedAgentRecord,
) -> bool {
    same_nonvolatile_config(current, attempted) || same_nonvolatile_config(current, original)
}

fn restore_config_preserving_runtime_state(
    current: &mut ManagedAgentRecord,
    original: &ManagedAgentRecord,
) {
    let runtime_pid = current.runtime_pid;
    let updated_at = current.updated_at.clone();
    let last_started_at = current.last_started_at.clone();
    let last_stopped_at = current.last_stopped_at.clone();
    let last_exit_code = current.last_exit_code;
    let last_error = current.last_error.clone();
    let last_error_code = current.last_error_code;
    *current = original.clone();
    current.runtime_pid = runtime_pid;
    current.updated_at = updated_at;
    current.last_started_at = last_started_at;
    current.last_stopped_at = last_stopped_at;
    current.last_exit_code = last_exit_code;
    current.last_error = last_error;
    current.last_error_code = last_error_code;
}

pub(crate) async fn restart_original_pairs(
    app: &AppHandle,
    original_relays: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    for (pubkey, relays) in original_relays {
        for relay in relays {
            match start_managed_agent_runtime_pair_lazy(pubkey.clone(), relay.clone(), app.clone())
            {
                Ok(_) => {
                    if let Err(error) = wait_for_ready(app, pubkey, relay).await {
                        errors.push((pubkey.clone(), error));
                    }
                }
                Err(error) => errors.push((pubkey.clone(), error)),
            }
        }
    }
    errors
}

fn runtime_keys_for_selected(
    runtimes: &HashMap<ManagedAgentRuntimeKey, crate::managed_agents::ManagedAgentPairRuntime>,
    selected: &[String],
) -> Vec<ManagedAgentRuntimeKey> {
    let selected: HashSet<&str> = selected.iter().map(String::as_str).collect();
    let mut keys: Vec<_> = runtimes
        .keys()
        .filter(|key| selected.contains(key.pubkey.as_str()))
        .cloned()
        .collect();
    keys.sort_by(|left, right| {
        left.pubkey
            .cmp(&right.pubkey)
            .then_with(|| left.relay_url.cmp(&right.relay_url))
    });
    keys
}

async fn drain_claimed_runtime_keys(
    app: &AppHandle,
    generations: &[JournalRuntimeGeneration],
    operation_id: &str,
) -> Vec<(
    ManagedAgentRuntimeKey,
    Result<crate::managed_agents::DrainedManagedAgentPair, ManagedAgentUpdateDrainError>,
)> {
    join_all(generations.iter().cloned().map(|generation| {
        let app = app.clone();
        let operation_id = operation_id.to_string();
        async move {
            let identity = PlannedUpdateIdentity {
                handoff_id: generation.handoff.handoff_id,
                start_nonce: generation.handoff.start_nonce,
            };
            let key = generation.key;
            let result =
                drain_managed_agent_pair_for_update(&app, &key, &operation_id, &identity).await;
            (key, result)
        }
    }))
    .await
}

fn clear_rollback_safe_claims(
    app: &AppHandle,
    operation_id: &str,
    drains: &[(
        ManagedAgentRuntimeKey,
        Result<crate::managed_agents::DrainedManagedAgentPair, ManagedAgentUpdateDrainError>,
    )],
) -> Result<(), String> {
    let keys: Vec<_> = drains
        .iter()
        .filter_map(|(key, result)| {
            result
                .as_ref()
                .err()
                .filter(|error| error.rollback_safe)
                .map(|_| key.clone())
        })
        .collect();
    if keys.is_empty() {
        return Ok(());
    }
    let state = app.state::<AppState>();
    let _transition = state
        .managed_agent_runtime_transition
        .lock()
        .map_err(|error| error.to_string())?;
    let mut runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;
    clear_managed_agent_runtime_pair_claims(&mut runtimes, &keys, operation_id)
}

fn clear_runtime_claims(
    app: &AppHandle,
    keys: &[ManagedAgentRuntimeKey],
    operation_id: &str,
) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    let state = app.state::<AppState>();
    let _transition = state
        .managed_agent_runtime_transition
        .lock()
        .map_err(|error| error.to_string())?;
    let mut runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;
    clear_managed_agent_runtime_pair_claims(&mut runtimes, keys, operation_id)
}

fn absent_drained_runtime_keys(
    app: &AppHandle,
    drains: &[(
        ManagedAgentRuntimeKey,
        Result<crate::managed_agents::DrainedManagedAgentPair, ManagedAgentUpdateDrainError>,
    )],
) -> Result<Vec<ManagedAgentRuntimeKey>, String> {
    let state = app.state::<AppState>();
    let runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;
    Ok(drains
        .iter()
        .filter(|(_, result)| result.is_ok())
        .filter(|(key, _)| !runtimes.contains_key(key))
        .map(|(key, _)| key.clone())
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainFailureState {
    OriginalStillRunning,
    ExitedWithoutCheckpoint,
    OwnershipUncertain,
}

fn classify_drain_failure(
    error: &ManagedAgentUpdateDrainError,
    runtime_is_tracked: bool,
) -> DrainFailureState {
    if !error.rollback_safe {
        DrainFailureState::OwnershipUncertain
    } else if runtime_is_tracked {
        DrainFailureState::OriginalStillRunning
    } else {
        DrainFailureState::ExitedWithoutCheckpoint
    }
}

fn rollback_safe_failure_lost_runtime(
    app: &AppHandle,
    drains: &[(
        ManagedAgentRuntimeKey,
        Result<crate::managed_agents::DrainedManagedAgentPair, ManagedAgentUpdateDrainError>,
    )],
) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;
    Ok(drains.iter().any(|(key, result)| {
        result.as_ref().err().is_some_and(|error| {
            classify_drain_failure(error, runtimes.contains_key(key))
                == DrainFailureState::ExitedWithoutCheckpoint
        })
    }))
}

async fn restart_runtime_keys(
    app: &AppHandle,
    keys: &[ManagedAgentRuntimeKey],
    operation_id: &str,
) -> Vec<(ManagedAgentRuntimeKey, String)> {
    let mut errors = Vec::new();
    for key in keys {
        match start_managed_agent_runtime_pair_authorized(
            key.pubkey.clone(),
            key.relay_url.clone(),
            true,
            operation_id,
            app.clone(),
        ) {
            Ok(_) => {
                if let Err(error) = wait_for_ready(app, &key.pubkey, &key.relay_url).await {
                    errors.push((key.clone(), error));
                }
            }
            Err(error) => errors.push((key.clone(), error)),
        }
    }
    errors
}

fn failure_response(
    persona_id: String,
    version: AgentTemplateVersionRef,
    selected: &[String],
    names: &BTreeMap<String, String>,
    rolled_back: bool,
    outcome: AgentTemplateUpdateOutcome,
    error: String,
) -> ApplyAgentTemplateUpdateResponse {
    let agents = selected
        .iter()
        .map(|pubkey| AgentTemplateUpdateResult {
            pubkey: pubkey.clone(),
            name: names.get(pubkey).cloned().unwrap_or_else(|| pubkey.clone()),
            outcome: match outcome {
                AgentTemplateUpdateOutcome::Updated => AgentTemplateUpdateOutcome::Updated,
                AgentTemplateUpdateOutcome::UpdatedStopped => {
                    AgentTemplateUpdateOutcome::UpdatedStopped
                }
                AgentTemplateUpdateOutcome::RolledBack => AgentTemplateUpdateOutcome::RolledBack,
                AgentTemplateUpdateOutcome::RollbackFailed => {
                    AgentTemplateUpdateOutcome::RollbackFailed
                }
            },
            error: Some(error.clone()),
        })
        .collect();
    ApplyAgentTemplateUpdateResponse {
        persona_id,
        version,
        rolled_back,
        agents,
    }
}

fn describe_drain_errors(
    drains: &[(
        ManagedAgentRuntimeKey,
        Result<crate::managed_agents::DrainedManagedAgentPair, ManagedAgentUpdateDrainError>,
    )],
) -> String {
    drains
        .iter()
        .filter_map(|(key, result)| {
            result
                .as_ref()
                .err()
                .map(|error| format!("{} on {}: {}", key.pubkey, key.relay_url, error.message))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn advance_update_transaction(
    journal: &UpdateTransactionJournal,
    transaction: &mut UpdateTransaction,
    stage: UpdateTransactionStage,
    candidate_generations: Vec<JournalRuntimeGeneration>,
) -> Result<(), String> {
    *transaction = journal.update(
        &transaction.transaction_id,
        transaction.revision,
        stage,
        candidate_generations,
    )?;
    Ok(())
}

fn retain_interrupted_update_block(
    state: &AppState,
    transaction: &UpdateTransaction,
) -> Result<(), String> {
    state
        .managed_agent_update_leases
        .block_for_recovery(
            &transaction.transaction_id,
            transaction.selected.keys().map(String::as_str),
        )
        .or_else(|error| {
            state
                .managed_agent_update_leases
                .block_all_for_recovery("recovery-global")
                .map_err(|global_error| {
                    format!("{error} Global recovery fencing also failed: {global_error}")
                })
        })
}

fn cleanup_unstarted_update_journal(
    state: &AppState,
    journal: &UpdateTransactionJournal,
    transaction_id: &str,
) {
    match journal.load(transaction_id) {
        Ok(transaction) if transaction.stage == UpdateTransactionStage::Prepared => {
            if journal
                .delete(&transaction.transaction_id, transaction.revision)
                .is_err()
            {
                let _ = state
                    .managed_agent_update_leases
                    .block_all_for_recovery("recovery-global");
            }
        }
        Ok(transaction) => {
            let _ = retain_interrupted_update_block(state, &transaction);
        }
        Err(_) => {
            let _ = state
                .managed_agent_update_leases
                .block_all_for_recovery("recovery-global");
        }
    }
}

#[tauri::command]
pub async fn apply_agent_template_update(
    input: ApplyAgentTemplateUpdateRequest,
    app: AppHandle,
) -> Result<ApplyAgentTemplateUpdateResponse, String> {
    validate_update_request_id(&input.request_id)?;
    emit_update_progress(
        &app,
        &input.request_id,
        AgentTemplateUpdateProgressStage::PreparingUpdate,
    );
    let state = app.state::<AppState>();
    let (persona_head, target_version) = {
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let personas = load_personas(&app)?;
        let persona = personas
            .iter()
            .find(|persona| persona.id == input.persona_id)
            .cloned()
            .ok_or_else(|| format!("Template {} no longer exists.", input.persona_id))?;
        let target_version = checked_published_version(&persona, &input.expected_version)?;
        (persona, target_version)
    };
    let load_app = app.clone();
    let load_persona = persona_head.clone();
    let load_version = target_version.clone();
    let target_persona = tokio::task::spawn_blocking(move || {
        load_agent_template_version(&load_app, &load_persona, &load_version)
    })
    .await
    .map_err(|error| format!("Template version task failed: {error}"))??;
    let target_version_token = target_version.authority_token()?;
    let operation_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let _update_lease = state
        .managed_agent_update_leases
        .try_acquire(
            operation_id.clone(),
            input.selected_pubkeys.iter().map(String::as_str),
        )
        .map_err(|error| format!("Could not begin the agent update: {error}"))?;

    // Stage the exact target without changing a record. Claiming all live
    // generations in the same transition prevents a normal stop or
    // replacement from racing the handoff.
    let (selected, originals, attempted, original_generations, names) = {
        let _transition = state
            .managed_agent_runtime_transition
            .lock()
            .map_err(|error| error.to_string())?;
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut personas = load_personas(&app)?;
        let persona = personas
            .iter()
            .find(|persona| persona.id == input.persona_id)
            .cloned()
            .ok_or_else(|| format!("Template {} no longer exists.", input.persona_id))?;
        checked_published_version(&persona, &input.expected_version)?;
        if let Some(slot) = personas
            .iter_mut()
            .find(|candidate| candidate.id == input.persona_id)
        {
            *slot = target_persona.clone();
        }
        let global = load_global_agent_config(&app).unwrap_or_default();
        let records = load_managed_agents(&app)?;
        let selected = validate_selection(&records, &input.persona_id, &input.selected_pubkeys)?;
        if input
            .connection_bindings_by_pubkey
            .keys()
            .any(|pubkey| !selected.iter().any(|selected| selected == pubkey))
        {
            return Err(
                "The connection choices include an agent that is not in this update.".to_string(),
            );
        }

        let mut originals = BTreeMap::new();
        let mut attempted = BTreeMap::new();
        let mut names = BTreeMap::new();
        for pubkey in &selected {
            let record = records
                .iter()
                .find(|record| record.pubkey.eq_ignore_ascii_case(pubkey))
                .ok_or_else(|| format!("Agent {pubkey} no longer exists."))?;
            prospective_readiness(record, &target_persona, &personas, &global)?;
            let bindings = input
                .connection_bindings_by_pubkey
                .get(pubkey)
                .cloned()
                .unwrap_or_else(|| retained_bindings(record, &target_persona));
            let mut prospective = record.clone();
            crate::managed_agents::persona_events::advance_persona_snapshot(
                &mut prospective,
                &target_persona,
            )?;
            prospective.persona_source_version = Some(target_version_token.clone());
            prospective.connection_bindings = bindings;
            crate::managed_agents::project_connections::validate_agent_project_connections(
                &app,
                &prospective,
            )?;
            originals.insert(pubkey.clone(), record.clone());
            attempted.insert(pubkey.clone(), prospective);
            names.insert(pubkey.clone(), record.name.clone());
        }

        let mut runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;
        let original_keys = runtime_keys_for_selected(&runtimes, &selected);
        let original_generations = original_keys
            .iter()
            .map(|key| {
                let runtime = runtimes.get(key).ok_or_else(|| {
                    "A running agent disappeared before it was claimed.".to_string()
                })?;
                let identity = PlannedUpdateIdentity::new(&runtime.start_nonce)?;
                Ok(JournalRuntimeGeneration {
                    key: key.clone(),
                    start_nonce: runtime.start_nonce.clone(),
                    handoff: JournalHandoffIdentity {
                        handoff_id: identity.handoff_id,
                        start_nonce: identity.start_nonce,
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        claim_managed_agent_runtime_pairs(&mut runtimes, &original_keys, &operation_id)
            .map_err(|error| format!("Could not begin the agent update: {error}"))?;
        (selected, originals, attempted, original_generations, names)
    };
    let original_keys = original_generations
        .iter()
        .map(|generation| generation.key.clone())
        .collect::<Vec<_>>();
    let selected_records = selected
        .iter()
        .map(|pubkey| {
            Ok((
                pubkey.clone(),
                SelectedAgentRecords {
                    original: originals
                        .get(pubkey)
                        .cloned()
                        .ok_or_else(|| format!("Agent {pubkey} has no original snapshot."))?,
                    attempted: attempted
                        .get(pubkey)
                        .cloned()
                        .ok_or_else(|| format!("Agent {pubkey} has no attempted snapshot."))?,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let mut transaction = UpdateTransaction::new(
        operation_id.clone(),
        input.request_id.clone(),
        ImmutableUpdateTarget {
            persona_id: input.persona_id.clone(),
            version: target_version.clone(),
        },
        selected_records,
        original_generations.clone(),
    )?;
    let journal = match UpdateTransactionJournal::for_app(&app) {
        Ok(journal) => journal,
        Err(error) => {
            let _ = clear_runtime_claims(&app, &original_keys, &operation_id);
            let _ = state
                .managed_agent_update_leases
                .block_all_for_recovery("recovery-global");
            return Err(error);
        }
    };
    if let Err(error) = journal.create(&transaction) {
        let _ = clear_runtime_claims(&app, &original_keys, &operation_id);
        cleanup_unstarted_update_journal(&state, &journal, &transaction.transaction_id);
        return Err(error);
    }
    if let Err(error) = advance_update_transaction(
        &journal,
        &mut transaction,
        UpdateTransactionStage::BeforeOriginalHandoff,
        Vec::new(),
    ) {
        let _ = clear_runtime_claims(&app, &original_keys, &operation_id);
        cleanup_unstarted_update_journal(&state, &journal, &transaction.transaction_id);
        return Err(format!(
            "Buzz could not make the update handoff durable: {error}"
        ));
    }

    if !original_keys.is_empty() {
        emit_update_progress(
            &app,
            &input.request_id,
            AgentTemplateUpdateProgressStage::FinishingCurrentTask,
        );
    }
    let original_drains =
        drain_claimed_runtime_keys(&app, &original_generations, &operation_id).await;
    if original_drains.iter().any(|(_, result)| result.is_err()) {
        let drain_error = describe_drain_errors(&original_drains);
        let mut recovery_errors = Vec::new();
        if let Err(error) = clear_rollback_safe_claims(&app, &operation_id, &original_drains) {
            recovery_errors.push(format!("Could not release the safe update claims: {error}"));
        }
        let unsafe_handoff = original_drains.iter().any(|(_, result)| {
            result
                .as_ref()
                .err()
                .is_some_and(|error| !error.rollback_safe)
        });
        let lost_without_checkpoint =
            match rollback_safe_failure_lost_runtime(&app, &original_drains) {
                Ok(lost) => lost,
                Err(error) => {
                    recovery_errors.push(error);
                    true
                }
            };
        if lost_without_checkpoint {
            recovery_errors.push(
                "An agent exited before writing a recovery checkpoint. Buzz left it stopped instead of restarting from uncertain state."
                    .to_string(),
            );
        }
        let restart_keys = match absent_drained_runtime_keys(&app, &original_drains) {
            Ok(keys) => keys,
            Err(error) => {
                recovery_errors.push(error);
                Vec::new()
            }
        };
        if !unsafe_handoff && !lost_without_checkpoint && recovery_errors.is_empty() {
            if let Err(error) = advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::BeforeOriginalRestart,
                Vec::new(),
            ) {
                recovery_errors.push(format!(
                    "Could not record the previous-version restart: {error}"
                ));
            } else {
                recovery_errors.extend(
                    restart_runtime_keys(&app, &restart_keys, &operation_id)
                        .await
                        .into_iter()
                        .map(|(key, error)| {
                            format!(
                                "Could not restart {} on {}: {error}",
                                key.pubkey, key.relay_url
                            )
                        }),
                );
                if recovery_errors.is_empty() {
                    if let Err(error) = advance_update_transaction(
                        &journal,
                        &mut transaction,
                        UpdateTransactionStage::AfterOriginalRestart,
                        Vec::new(),
                    )
                    .and_then(|()| {
                        advance_update_transaction(
                            &journal,
                            &mut transaction,
                            UpdateTransactionStage::RollbackCompleted,
                            Vec::new(),
                        )
                    }) {
                        recovery_errors
                            .push(format!("Could not finish the rollback journal: {error}"));
                    } else if let Err(error) =
                        journal.delete(&transaction.transaction_id, transaction.revision)
                    {
                        eprintln!(
                            "buzz-desktop: completed update journal cleanup deferred: {error}"
                        );
                    }
                }
            }
        }
        let safely_recovered =
            !unsafe_handoff && !lost_without_checkpoint && recovery_errors.is_empty();
        if !safely_recovered {
            let _ = retain_interrupted_update_block(&state, &transaction);
        }
        emit_update_progress(
            &app,
            &input.request_id,
            if safely_recovered {
                AgentTemplateUpdateProgressStage::UpdateRolledBack
            } else {
                AgentTemplateUpdateProgressStage::NeedsAttention
            },
        );
        let mut error = format!("Buzz could not finish preparing the agents: {drain_error}");
        if !recovery_errors.is_empty() {
            error.push_str(" Recovery also failed: ");
            error.push_str(&recovery_errors.join("; "));
        }
        return Ok(failure_response(
            input.persona_id,
            target_version,
            &selected,
            &names,
            safely_recovered,
            if safely_recovered {
                AgentTemplateUpdateOutcome::RolledBack
            } else {
                AgentTemplateUpdateOutcome::RollbackFailed
            },
            error,
        ));
    }

    for stage in [
        UpdateTransactionStage::AfterOriginalHandoff,
        UpdateTransactionStage::BeforeCandidateCommit,
    ] {
        if let Err(error) =
            advance_update_transaction(&journal, &mut transaction, stage, Vec::new())
        {
            let _ = retain_interrupted_update_block(&state, &transaction);
            return Err(format!(
                "Buzz stopped the update because its recovery journal could not advance: {error}"
            ));
        }
    }

    // The originals have exited with verified checkpoints. Recheck the
    // immutable reference and every selected record before one atomic save
    // makes the staged version current.
    let commit_result = (|| -> Result<(), String> {
        let _transition = state
            .managed_agent_runtime_transition
            .lock()
            .map_err(|error| error.to_string())?;
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let personas = load_personas(&app)?;
        let persona = personas
            .iter()
            .find(|persona| persona.id == input.persona_id)
            .ok_or_else(|| format!("Template {} no longer exists.", input.persona_id))?;
        checked_published_version(persona, &input.expected_version)?;
        let mut records = load_managed_agents(&app)?;
        for (pubkey, original) in &originals {
            let current = records
                .iter()
                .find(|record| record.pubkey.eq_ignore_ascii_case(pubkey))
                .ok_or_else(|| format!("Agent {pubkey} no longer exists."))?;
            if !same_nonvolatile_config(current, original) {
                return Err(format!(
                    "{} changed while the update was being prepared. Review the affected agents and try again.",
                    current.name
                ));
            }
        }
        let runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;
        if !runtime_keys_for_selected(&runtimes, &selected).is_empty() {
            return Err(
                "An affected agent started while the update was being prepared. Its configuration was not changed."
                    .to_string(),
            );
        }
        drop(runtimes);

        let now = crate::util::now_iso();
        for pubkey in &selected {
            let current = find_managed_agent_mut(&mut records, pubkey)?;
            let next = attempted
                .get(pubkey)
                .ok_or_else(|| format!("Agent {pubkey} has no staged update."))?;
            *current = next.clone();
            current.runtime_pid = None;
            current.updated_at = now.clone();
            current.last_stopped_at = Some(now.clone());
            current.last_error = None;
            current.last_error_code = None;
        }
        save_managed_agents_for_operation(&app, &records, &operation_id)
    })();

    if let Err(error) = commit_result {
        let mut recovery_errors = Vec::new();
        if let Err(journal_error) = advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::BeforeOriginalRestore,
            Vec::new(),
        ) {
            recovery_errors.push(format!(
                "Could not record the original configuration restore: {journal_error}"
            ));
        } else {
            let restore_result =
                restore_original_records(&app, &selected, &originals, &attempted, &operation_id);
            if let Err(restore_error) = restore_result {
                recovery_errors.push(format!(
                    "Could not restore the original configuration: {restore_error}"
                ));
            } else if let Err(journal_error) = advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::AfterOriginalRestore,
                Vec::new(),
            )
            .and_then(|()| {
                advance_update_transaction(
                    &journal,
                    &mut transaction,
                    UpdateTransactionStage::BeforeOriginalRestart,
                    Vec::new(),
                )
            }) {
                recovery_errors.push(format!(
                    "Could not record the original configuration recovery: {journal_error}"
                ));
            } else {
                recovery_errors.extend(
                    restart_runtime_keys(&app, &original_keys, &operation_id)
                        .await
                        .into_iter()
                        .map(|(key, restart_error)| {
                            format!(
                                "Could not restart {} on {}: {restart_error}",
                                key.pubkey, key.relay_url
                            )
                        }),
                );
                if recovery_errors.is_empty() {
                    if let Err(journal_error) = advance_update_transaction(
                        &journal,
                        &mut transaction,
                        UpdateTransactionStage::AfterOriginalRestart,
                        Vec::new(),
                    )
                    .and_then(|()| {
                        advance_update_transaction(
                            &journal,
                            &mut transaction,
                            UpdateTransactionStage::RollbackCompleted,
                            Vec::new(),
                        )
                    }) {
                        recovery_errors.push(format!(
                            "Could not finish the rollback journal: {journal_error}"
                        ));
                    } else if let Err(delete_error) =
                        journal.delete(&transaction.transaction_id, transaction.revision)
                    {
                        eprintln!(
                            "buzz-desktop: completed update journal cleanup deferred: {delete_error}"
                        );
                    }
                }
            }
        }
        let safely_recovered = recovery_errors.is_empty();
        if !safely_recovered {
            let _ = retain_interrupted_update_block(&state, &transaction);
        }
        emit_update_progress(
            &app,
            &input.request_id,
            if safely_recovered {
                AgentTemplateUpdateProgressStage::UpdateRolledBack
            } else {
                AgentTemplateUpdateProgressStage::NeedsAttention
            },
        );
        let mut message = error;
        if !recovery_errors.is_empty() {
            message.push_str(" Recovery also failed: ");
            message.push_str(&recovery_errors.join("; "));
        }
        return Ok(failure_response(
            input.persona_id,
            target_version,
            &selected,
            &names,
            safely_recovered,
            if safely_recovered {
                AgentTemplateUpdateOutcome::RolledBack
            } else {
                AgentTemplateUpdateOutcome::RollbackFailed
            },
            message,
        ));
    }

    if let Err(error) = advance_update_transaction(
        &journal,
        &mut transaction,
        UpdateTransactionStage::AfterCandidateCommit,
        Vec::new(),
    ) {
        let _ = retain_interrupted_update_block(&state, &transaction);
        return Err(format!(
            "Buzz saved the update but could not advance its recovery journal: {error}"
        ));
    }
    let candidate_generations = original_keys
        .iter()
        .map(|key| {
            let start_nonce = uuid::Uuid::new_v4().simple().to_string();
            let handoff = PlannedUpdateIdentity::new(&start_nonce)?;
            Ok(JournalRuntimeGeneration {
                key: key.clone(),
                start_nonce: start_nonce.clone(),
                handoff: JournalHandoffIdentity {
                    handoff_id: handoff.handoff_id,
                    start_nonce,
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if let Err(error) = advance_update_transaction(
        &journal,
        &mut transaction,
        UpdateTransactionStage::BeforeCandidateLaunch,
        candidate_generations.clone(),
    ) {
        let _ = retain_interrupted_update_block(&state, &transaction);
        return Err(format!(
            "Buzz did not start the updated agents because their process identities could not be made durable: {error}"
        ));
    }

    let mut update_error = None;
    for generation in &candidate_generations {
        let key = &generation.key;
        emit_update_progress(
            &app,
            &input.request_id,
            AgentTemplateUpdateProgressStage::StartingUpdatedAgent,
        );
        match start_managed_agent_runtime_pair_authorized_with_nonce(
            key.pubkey.clone(),
            key.relay_url.clone(),
            true,
            &operation_id,
            &generation.start_nonce,
            app.clone(),
        ) {
            Ok(_) => {
                emit_update_progress(
                    &app,
                    &input.request_id,
                    AgentTemplateUpdateProgressStage::CheckingUpdate,
                );
                if let Err(error) = wait_for_ready(&app, &key.pubkey, &key.relay_url).await {
                    update_error = Some(format!(
                        "{} on {} did not become available: {error}",
                        key.pubkey, key.relay_url
                    ));
                    break;
                }
            }
            Err(error) => {
                update_error = Some(format!(
                    "Could not start {} on {}: {error}",
                    key.pubkey, key.relay_url
                ));
                break;
            }
        }
    }

    if let Some(error) = update_error {
        let candidate_generations_to_drain = {
            let _transition = state
                .managed_agent_runtime_transition
                .lock()
                .map_err(|lock_error| lock_error.to_string())?;
            let mut runtimes = state
                .managed_agent_processes
                .lock()
                .map_err(|lock_error| lock_error.to_string())?;
            let keys = runtime_keys_for_selected(&runtimes, &selected);
            let generations = keys
                .iter()
                .map(|key| {
                    let planned = candidate_generations
                        .iter()
                        .find(|generation| generation.key == *key)
                        .ok_or_else(|| {
                            "An unplanned replacement runtime appeared during rollback.".to_string()
                        })?;
                    let runtime = runtimes.get(key).ok_or_else(|| {
                        "A replacement runtime disappeared before rollback.".to_string()
                    })?;
                    if runtime.start_nonce != planned.start_nonce {
                        return Err(
                            "A replacement runtime generation did not match the durable update plan."
                                .to_string(),
                        );
                    }
                    Ok(planned.clone())
                })
                .collect::<Result<Vec<_>, String>>();
            let generations = match generations {
                Ok(generations) => generations,
                Err(claim_error) => {
                    let _ = retain_interrupted_update_block(&state, &transaction);
                    emit_update_progress(
                        &app,
                        &input.request_id,
                        AgentTemplateUpdateProgressStage::NeedsAttention,
                    );
                    return Ok(failure_response(
                        input.persona_id,
                        target_version,
                        &selected,
                        &names,
                        false,
                        AgentTemplateUpdateOutcome::RollbackFailed,
                        format!(
                            "{error} Buzz could not verify every replacement agent before rollback: {claim_error}"
                        ),
                    ));
                }
            };
            if let Err(claim_error) =
                claim_managed_agent_runtime_pairs(&mut runtimes, &keys, &operation_id)
            {
                let _ = retain_interrupted_update_block(&state, &transaction);
                emit_update_progress(
                    &app,
                    &input.request_id,
                    AgentTemplateUpdateProgressStage::NeedsAttention,
                );
                return Ok(failure_response(
                    input.persona_id,
                    target_version,
                    &selected,
                    &names,
                    false,
                    AgentTemplateUpdateOutcome::RollbackFailed,
                    format!(
                        "{error} Buzz could not take safe ownership of every replacement agent: {claim_error}"
                    ),
                ));
            }
            generations
        };
        if let Err(journal_error) = advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::BeforeCandidateHandoff,
            candidate_generations.clone(),
        ) {
            let _ = retain_interrupted_update_block(&state, &transaction);
            return Err(format!(
                "Buzz could not make the replacement handoff durable: {journal_error}"
            ));
        }
        let candidate_drains =
            drain_claimed_runtime_keys(&app, &candidate_generations_to_drain, &operation_id).await;
        if candidate_drains.iter().any(|(_, result)| result.is_err()) {
            let drain_errors = describe_drain_errors(&candidate_drains);
            let _ = clear_rollback_safe_claims(&app, &operation_id, &candidate_drains);
            let _ = retain_interrupted_update_block(&state, &transaction);
            emit_update_progress(
                &app,
                &input.request_id,
                AgentTemplateUpdateProgressStage::NeedsAttention,
            );
            return Ok(failure_response(
                input.persona_id,
                target_version,
                &selected,
                &names,
                false,
                AgentTemplateUpdateOutcome::RollbackFailed,
                format!(
                    "{error} Buzz could not safely stop every replacement agent, so it did not start the previous version: {drain_errors}"
                ),
            ));
        }

        if let Err(journal_error) = advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::AfterCandidateHandoff,
            candidate_generations.clone(),
        )
        .and_then(|()| {
            advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::BeforeOriginalRestore,
                candidate_generations.clone(),
            )
        }) {
            let _ = retain_interrupted_update_block(&state, &transaction);
            return Err(format!(
                "Buzz stopped rollback because its recovery journal could not advance: {journal_error}"
            ));
        }

        if let Err(restore_error) =
            restore_original_records(&app, &selected, &originals, &attempted, &operation_id)
        {
            let _ = retain_interrupted_update_block(&state, &transaction);
            emit_update_progress(
                &app,
                &input.request_id,
                AgentTemplateUpdateProgressStage::NeedsAttention,
            );
            return Ok(failure_response(
                input.persona_id,
                target_version,
                &selected,
                &names,
                false,
                AgentTemplateUpdateOutcome::RollbackFailed,
                format!(
                    "{error} Buzz could not safely restore the previous configuration: {restore_error}"
                ),
            ));
        }
        if let Err(journal_error) = advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::AfterOriginalRestore,
            candidate_generations.clone(),
        )
        .and_then(|()| {
            advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::BeforeOriginalRestart,
                candidate_generations.clone(),
            )
        }) {
            let _ = retain_interrupted_update_block(&state, &transaction);
            return Err(format!(
                "Buzz restored the previous configuration but could not advance its recovery journal: {journal_error}"
            ));
        }
        let restart_errors = restart_runtime_keys(&app, &original_keys, &operation_id).await;
        if !restart_errors.is_empty() {
            let _ = retain_interrupted_update_block(&state, &transaction);
            emit_update_progress(
                &app,
                &input.request_id,
                AgentTemplateUpdateProgressStage::NeedsAttention,
            );
            return Ok(failure_response(
                input.persona_id,
                target_version,
                &selected,
                &names,
                false,
                AgentTemplateUpdateOutcome::RollbackFailed,
                format!(
                    "{error} The previous version was restored, but one or more agents could not restart: {}",
                    restart_errors
                        .into_iter()
                        .map(|(key, restart_error)| format!(
                            "{} on {}: {restart_error}",
                            key.pubkey, key.relay_url
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            ));
        }
        if let Err(journal_error) = advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::AfterOriginalRestart,
            candidate_generations.clone(),
        )
        .and_then(|()| {
            advance_update_transaction(
                &journal,
                &mut transaction,
                UpdateTransactionStage::RollbackCompleted,
                candidate_generations.clone(),
            )
        }) {
            let _ = retain_interrupted_update_block(&state, &transaction);
            return Err(format!(
                "Buzz restored the previous agents but could not finish its recovery journal: {journal_error}"
            ));
        }
        if let Err(delete_error) = journal.delete(&transaction.transaction_id, transaction.revision)
        {
            eprintln!("buzz-desktop: completed update journal cleanup deferred: {delete_error}");
        }
        emit_update_progress(
            &app,
            &input.request_id,
            AgentTemplateUpdateProgressStage::UpdateRolledBack,
        );
        return Ok(failure_response(
            input.persona_id,
            target_version,
            &selected,
            &names,
            true,
            AgentTemplateUpdateOutcome::RolledBack,
            error,
        ));
    }

    if let Err(journal_error) = advance_update_transaction(
        &journal,
        &mut transaction,
        UpdateTransactionStage::AfterCandidateReady,
        candidate_generations.clone(),
    )
    .and_then(|()| {
        advance_update_transaction(
            &journal,
            &mut transaction,
            UpdateTransactionStage::UpdateCompleted,
            candidate_generations.clone(),
        )
    }) {
        let _ = retain_interrupted_update_block(&state, &transaction);
        return Err(format!(
            "The updated agents are available, but Buzz could not finish their recovery journal: {journal_error}"
        ));
    }
    if let Err(delete_error) = journal.delete(&transaction.transaction_id, transaction.revision) {
        eprintln!("buzz-desktop: completed update journal cleanup deferred: {delete_error}");
    }

    let agents = selected
        .iter()
        .map(|pubkey| {
            let running = original_keys.iter().any(|key| key.pubkey == *pubkey);
            AgentTemplateUpdateResult {
                pubkey: pubkey.clone(),
                name: names.get(pubkey).cloned().unwrap_or_else(|| pubkey.clone()),
                outcome: if running {
                    AgentTemplateUpdateOutcome::Updated
                } else {
                    AgentTemplateUpdateOutcome::UpdatedStopped
                },
                error: None,
            }
        })
        .collect();
    emit_update_progress(
        &app,
        &input.request_id,
        AgentTemplateUpdateProgressStage::Updated,
    );
    Ok(ApplyAgentTemplateUpdateResponse {
        persona_id: input.persona_id,
        version: target_version,
        rolled_back: false,
        agents,
    })
}

#[cfg(test)]
#[path = "agent_template_updates/tests.rs"]
mod tests;
