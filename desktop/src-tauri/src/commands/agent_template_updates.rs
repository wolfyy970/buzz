use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    app_state::AppState,
    managed_agents::{
        agent_readiness, claim_managed_agent_runtime_pairs,
        clear_managed_agent_runtime_pair_claims, drain_managed_agent_pair_for_update,
        find_managed_agent_mut, known_acp_runtime, load_global_agent_config, load_managed_agents,
        load_personas, managed_agent_runtime_keys, resolve_effective_agent_env,
        save_managed_agents, start_managed_agent_runtime_pair_lazy, AgentReadiness,
        AgentTemplateVersionRef, BackendKind, ManagedAgentRecord, ManagedAgentRuntimeKey,
        ManagedAgentRuntimeLifecycle, ManagedAgentUpdateDrainError,
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
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateSkillChanges {
    pub added: Vec<crate::managed_agents::AgentSkill>,
    pub changed: Vec<AgentTemplateSkillChange>,
    pub removed: Vec<crate::managed_agents::AgentSkill>,
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
    pub tool_changes: AgentTemplateToolChanges,
    pub skill_changes: AgentTemplateSkillChanges,
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

fn restore_original_records(
    app: &AppHandle,
    selected: &[String],
    originals: &BTreeMap<String, ManagedAgentRecord>,
    attempted: &BTreeMap<String, ManagedAgentRecord>,
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
    save_managed_agents(app, &records)
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
    keys: &[ManagedAgentRuntimeKey],
    operation_id: &str,
) -> Vec<(
    ManagedAgentRuntimeKey,
    Result<crate::managed_agents::DrainedManagedAgentPair, ManagedAgentUpdateDrainError>,
)> {
    join_all(keys.iter().cloned().map(|key| {
        let app = app.clone();
        let operation_id = operation_id.to_string();
        async move {
            let result = drain_managed_agent_pair_for_update(&app, &key, &operation_id).await;
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
) -> Vec<(ManagedAgentRuntimeKey, String)> {
    let mut errors = Vec::new();
    for key in keys {
        match start_managed_agent_runtime_pair_lazy(
            key.pubkey.clone(),
            key.relay_url.clone(),
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
    let operation_id = uuid::Uuid::new_v4().simple().to_string();

    // Stage the exact target without changing a record. Claiming all live
    // generations in the same transition prevents a normal stop or
    // replacement from racing the handoff.
    let (selected, originals, attempted, original_keys, names) = {
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
        claim_managed_agent_runtime_pairs(&mut runtimes, &original_keys, &operation_id)
            .map_err(|error| format!("Could not begin the agent update: {error}"))?;
        (selected, originals, attempted, original_keys, names)
    };

    if !original_keys.is_empty() {
        emit_update_progress(
            &app,
            &input.request_id,
            AgentTemplateUpdateProgressStage::FinishingCurrentTask,
        );
    }
    let original_drains = drain_claimed_runtime_keys(&app, &original_keys, &operation_id).await;
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
        match absent_drained_runtime_keys(&app, &original_drains) {
            Ok(keys) => {
                recovery_errors.extend(restart_runtime_keys(&app, &keys).await.into_iter().map(
                    |(key, error)| {
                        format!(
                            "Could not restart {} on {}: {error}",
                            key.pubkey, key.relay_url
                        )
                    },
                ));
            }
            Err(error) => recovery_errors.push(error),
        }
        let safely_recovered =
            !unsafe_handoff && !lost_without_checkpoint && recovery_errors.is_empty();
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
        save_managed_agents(&app, &records)
    })();

    if let Err(error) = commit_result {
        let restore_result = restore_original_records(&app, &selected, &originals, &attempted);
        let restart_errors = if restore_result.is_ok() {
            restart_runtime_keys(&app, &original_keys).await
        } else {
            Vec::new()
        };
        let mut recovery_errors = Vec::new();
        if let Err(restore_error) = restore_result {
            recovery_errors.push(format!(
                "Could not restore the original configuration: {restore_error}"
            ));
        }
        recovery_errors.extend(restart_errors.into_iter().map(|(key, restart_error)| {
            format!(
                "Could not restart {} on {}: {restart_error}",
                key.pubkey, key.relay_url
            )
        }));
        let safely_recovered = recovery_errors.is_empty();
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

    let mut update_error = None;
    for key in &original_keys {
        emit_update_progress(
            &app,
            &input.request_id,
            AgentTemplateUpdateProgressStage::StartingUpdatedAgent,
        );
        match start_managed_agent_runtime_pair_lazy(
            key.pubkey.clone(),
            key.relay_url.clone(),
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
        // Every pair using the attempted configuration, including a
        // concurrently added relay pair, must be claimed and handed off before
        // the previous records can be restored.
        let rollback_operation_id = uuid::Uuid::new_v4().simple().to_string();
        let candidate_keys = {
            let _transition = state
                .managed_agent_runtime_transition
                .lock()
                .map_err(|lock_error| lock_error.to_string())?;
            let mut runtimes = state
                .managed_agent_processes
                .lock()
                .map_err(|lock_error| lock_error.to_string())?;
            let keys = runtime_keys_for_selected(&runtimes, &selected);
            if let Err(claim_error) =
                claim_managed_agent_runtime_pairs(&mut runtimes, &keys, &rollback_operation_id)
            {
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
            keys
        };
        let candidate_drains =
            drain_claimed_runtime_keys(&app, &candidate_keys, &rollback_operation_id).await;
        if candidate_drains.iter().any(|(_, result)| result.is_err()) {
            let drain_errors = describe_drain_errors(&candidate_drains);
            let _ = clear_rollback_safe_claims(&app, &rollback_operation_id, &candidate_drains);
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

        if let Err(restore_error) =
            restore_original_records(&app, &selected, &originals, &attempted)
        {
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
        let restart_errors = restart_runtime_keys(&app, &original_keys).await;
        if !restart_errors.is_empty() {
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
