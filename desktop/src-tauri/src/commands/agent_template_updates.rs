use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{
    app_state::AppState,
    managed_agents::{
        agent_readiness, find_managed_agent_mut, known_acp_runtime, load_global_agent_config,
        load_managed_agents, load_personas, managed_agent_runtime_keys,
        resolve_effective_agent_env, save_managed_agents, start_managed_agent_runtime_pair_lazy,
        stop_managed_agent_process, AgentReadiness, BackendKind, ManagedAgentRecord,
        ManagedAgentRuntimeKey, ManagedAgentRuntimeLifecycle,
    },
};

const UPDATE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_READY_POLL: Duration = Duration::from_millis(200);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAgentTemplateUpdateRequest {
    pub persona_id: String,
    pub expected_version: String,
    pub selected_pubkeys: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdatePreview {
    pub persona_id: String,
    pub persona_name: String,
    pub target_version: String,
    pub agents: Vec<AgentTemplateUpdateTarget>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdateTarget {
    pub pubkey: String,
    pub name: String,
    pub current_version: Option<String>,
    pub target_version: String,
    pub running_relays: Vec<String>,
    pub eligible: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTemplateUpdateOutcome {
    Updated,
    UpdatedStopped,
    RolledBack,
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
    pub version: String,
    pub rolled_back: bool,
    pub agents: Vec<AgentTemplateUpdateResult>,
}

fn persona_version(persona: &crate::managed_agents::AgentDefinition) -> String {
    crate::managed_agents::persona_events::persona_snapshot_version(persona)
}

fn checked_persona_version(
    persona: &crate::managed_agents::AgentDefinition,
    expected_version: &str,
) -> Result<String, String> {
    let current_version = persona_version(persona);
    if current_version != expected_version {
        return Err(
            "This template changed while you were reviewing it. Review the affected agents again."
                .to_string(),
        );
    }
    Ok(current_version)
}

fn running_relays_for(
    runtimes: &std::collections::HashMap<
        ManagedAgentRuntimeKey,
        crate::managed_agents::ManagedAgentPairRuntime,
    >,
    pubkey: &str,
) -> Vec<String> {
    let mut relays: Vec<String> = managed_agent_runtime_keys(runtimes, pubkey)
        .into_iter()
        .map(|key| key.relay_url)
        .collect();
    relays.sort();
    relays
}

fn template_update_blocked_reason(
    backend: &BackendKind,
    readiness_error: Option<String>,
) -> Option<String> {
    if backend != &BackendKind::Local {
        return Some("Remote agents cannot be updated safely in this version.".to_string());
    }
    readiness_error
}

fn validate_selection(
    records: &[ManagedAgentRecord],
    persona_id: &str,
    selected_pubkeys: &[String],
) -> Result<Vec<String>, String> {
    if selected_pubkeys.is_empty() {
        return Err("Choose at least one agent to update.".to_string());
    }
    let mut seen = HashSet::new();
    for pubkey in selected_pubkeys {
        let normalized_pubkey = pubkey.trim().to_ascii_lowercase();
        if !seen.insert(normalized_pubkey.clone()) {
            return Err(format!(
                "Agent {normalized_pubkey} was selected more than once."
            ));
        }
    }
    let mut normalized = Vec::with_capacity(selected_pubkeys.len());
    for pubkey in selected_pubkeys {
        let normalized_pubkey = pubkey.trim().to_ascii_lowercase();
        let record = records
            .iter()
            .find(|record| record.pubkey.eq_ignore_ascii_case(&normalized_pubkey))
            .ok_or_else(|| format!("Agent {normalized_pubkey} no longer exists."))?;
        if record.persona_id.as_deref() != Some(persona_id) {
            return Err(format!(
                "{} no longer uses this template. Review the affected agents and try again.",
                record.name
            ));
        }
        if record.backend != BackendKind::Local {
            return Err(format!(
                "{} is managed by a remote provider and cannot be updated safely yet.",
                record.name
            ));
        }
        normalized.push(record.pubkey.clone());
    }
    Ok(normalized)
}

#[tauri::command]
pub fn preview_agent_template_update(
    persona_id: String,
    app: AppHandle,
) -> Result<AgentTemplateUpdatePreview, String> {
    let state = app.state::<AppState>();
    let _store = state
        .managed_agents_store_lock
        .lock()
        .map_err(|error| error.to_string())?;
    let records = load_managed_agents(&app)?;
    let personas = load_personas(&app)?;
    let persona = personas
        .iter()
        .find(|persona| persona.id == persona_id)
        .ok_or_else(|| format!("Template {persona_id} no longer exists."))?;
    let target_version = persona_version(persona);
    let global = load_global_agent_config(&app).unwrap_or_default();
    let preview_rows: Vec<(&ManagedAgentRecord, Option<String>)> = records
        .iter()
        .filter(|record| record.persona_id.as_deref() == Some(persona_id.as_str()))
        .map(|record| {
            let readiness_error = (record.backend == BackendKind::Local)
                .then(|| prospective_readiness(record, persona, &personas, &global).err())
                .flatten();
            (
                record,
                template_update_blocked_reason(&record.backend, readiness_error),
            )
        })
        .collect();
    let runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;

    let mut agents: Vec<AgentTemplateUpdateTarget> = preview_rows
        .into_iter()
        .map(|(record, blocked_reason)| AgentTemplateUpdateTarget {
            pubkey: record.pubkey.clone(),
            name: record.name.clone(),
            current_version: record.persona_source_version.clone(),
            target_version: target_version.clone(),
            running_relays: running_relays_for(&runtimes, &record.pubkey),
            eligible: blocked_reason.is_none(),
            blocked_reason,
        })
        .collect();
    agents.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.pubkey.cmp(&right.pubkey))
    });

    Ok(AgentTemplateUpdatePreview {
        persona_id,
        persona_name: persona.display_name.clone(),
        target_version,
        agents,
    })
}

fn prospective_readiness(
    record: &ManagedAgentRecord,
    persona: &crate::managed_agents::AgentDefinition,
    personas: &[crate::managed_agents::AgentDefinition],
    global: &crate::managed_agents::GlobalAgentConfig,
) -> Result<(), String> {
    let mut prospective = record.clone();
    crate::managed_agents::persona_events::apply_persona_snapshot(&mut prospective, persona)?;
    let command = crate::managed_agents::record_agent_command(&prospective, personas);
    let runtime = known_acp_runtime(&command);
    let effective = resolve_effective_agent_env(&prospective, personas, runtime, global);
    match agent_readiness(&effective) {
        AgentReadiness::Ready => Ok(()),
        AgentReadiness::NotReady { requirements } => Err(format!(
            "{} cannot use this template yet: {}",
            record.name,
            format_readiness_requirements(&requirements)
        )),
    }
}

fn format_readiness_requirements(requirements: &[crate::managed_agents::Requirement]) -> String {
    if requirements.is_empty() {
        return "complete the required agent setup".to_string();
    }
    requirements
        .iter()
        .map(|requirement| match requirement {
            crate::managed_agents::Requirement::PersonaSnapshotUninitialized => {
                "restart Buzz to finish migrating this agent".to_string()
            }
            crate::managed_agents::Requirement::NormalizedField { field } => match field.as_str() {
                "provider" => "choose a provider".to_string(),
                "model" => "choose a model".to_string(),
                _ => format!("configure {field}"),
            },
            crate::managed_agents::Requirement::EnvKey { key } => {
                format!("set the required {key} environment variable")
            }
            crate::managed_agents::Requirement::CliLogin { setup_copy, .. } => setup_copy.clone(),
            crate::managed_agents::Requirement::CliConfigInvalid { probe_args, .. } => {
                let cli = probe_args.first().map(String::as_str).unwrap_or("agent");
                format!("repair the {cli} CLI configuration")
            }
            crate::managed_agents::Requirement::GitBash => {
                "install Git Bash for the agent shell tools".to_string()
            }
            crate::managed_agents::Requirement::MissingBinary { command } => {
                format!("install {command} or add it to PATH")
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

async fn wait_for_ready(app: &AppHandle, pubkey: &str, relay_url: &str) -> Result<(), String> {
    let key = ManagedAgentRuntimeKey::new(pubkey.to_string(), relay_url)?;
    let started = Instant::now();
    loop {
        {
            let state = app.state::<AppState>();
            let mut runtimes = state
                .managed_agent_processes
                .lock()
                .map_err(|error| error.to_string())?;
            let runtime = runtimes
                .get_mut(&key)
                .ok_or_else(|| "The updated agent stopped before it became ready.".to_string())?;
            if let Some(status) = runtime
                .child
                .try_wait()
                .map_err(|error| format!("Could not check the updated agent: {error}"))?
            {
                return Err(format!(
                    "The updated agent exited before it became ready ({status})."
                ));
            }
            match runtime.lifecycle {
                ManagedAgentRuntimeLifecycle::Ready => return Ok(()),
                ManagedAgentRuntimeLifecycle::Failed => {
                    return Err(runtime.error.clone().unwrap_or_else(|| {
                        "The updated agent failed its readiness check.".into()
                    }));
                }
                ManagedAgentRuntimeLifecycle::Starting
                | ManagedAgentRuntimeLifecycle::Listening
                | ManagedAgentRuntimeLifecycle::Waking
                | ManagedAgentRuntimeLifecycle::Stopped => {}
            }
        }
        if started.elapsed() >= UPDATE_READY_TIMEOUT {
            return Err("The updated agent did not become ready within 30 seconds.".to_string());
        }
        tokio::time::sleep(UPDATE_READY_POLL).await;
    }
}

fn restore_original_records(
    app: &AppHandle,
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

async fn restart_original_pairs(
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

fn append_error(target: &mut Option<String>, error: String) {
    match target {
        Some(existing) => {
            existing.push(' ');
            existing.push_str(&error);
        }
        None => *target = Some(error),
    }
}

#[tauri::command]
pub async fn apply_agent_template_update(
    input: ApplyAgentTemplateUpdateRequest,
    app: AppHandle,
) -> Result<ApplyAgentTemplateUpdateResponse, String> {
    let state = app.state::<AppState>();

    let (selected, originals, attempted, original_relays, names, prepare_error, target_version) = {
        let _transition = state
            .managed_agent_runtime_transition
            .lock()
            .map_err(|error| error.to_string())?;
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        // Persona writes use the same store lock. Read, validate, and retain
        // the exact head under that lock so an edit cannot race between the
        // optimistic version check and the snapshot actually installed.
        let personas = load_personas(&app)?;
        let persona = personas
            .iter()
            .find(|persona| persona.id == input.persona_id)
            .cloned()
            .ok_or_else(|| format!("Template {} no longer exists.", input.persona_id))?;
        let target_version = checked_persona_version(&persona, &input.expected_version)?;
        let global = load_global_agent_config(&app).unwrap_or_default();
        let mut records = load_managed_agents(&app)?;
        let selected = validate_selection(&records, &input.persona_id, &input.selected_pubkeys)?;

        for pubkey in &selected {
            let record = records
                .iter()
                .find(|record| record.pubkey.eq_ignore_ascii_case(pubkey))
                .ok_or_else(|| format!("Agent {pubkey} no longer exists."))?;
            prospective_readiness(record, &persona, &personas, &global)?;
        }

        let mut runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;
        let mut originals = BTreeMap::new();
        let mut original_relays = BTreeMap::new();
        let mut names = BTreeMap::new();

        for pubkey in &selected {
            let record = find_managed_agent_mut(&mut records, pubkey)?;
            originals.insert(pubkey.clone(), record.clone());
            names.insert(pubkey.clone(), record.name.clone());
            original_relays.insert(
                pubkey.clone(),
                running_relays_for(&runtimes, &record.pubkey),
            );
        }
        let mut prepare_error = None;
        for pubkey in &selected {
            let record = match find_managed_agent_mut(&mut records, pubkey) {
                Ok(record) => record,
                Err(error) => {
                    prepare_error = Some(error);
                    break;
                }
            };
            if let Err(error) = stop_managed_agent_process(&app, record, &mut runtimes) {
                prepare_error = Some(format!(
                    "Could not prepare {} for update: {error}",
                    record.name
                ));
                break;
            }
        }
        if prepare_error.is_none() {
            for pubkey in &selected {
                let record = match find_managed_agent_mut(&mut records, pubkey) {
                    Ok(record) => record,
                    Err(error) => {
                        prepare_error = Some(error);
                        break;
                    }
                };
                if let Err(error) = crate::managed_agents::persona_events::advance_persona_snapshot(
                    record, &persona,
                ) {
                    prepare_error = Some(format!("Could not update {}: {error}", record.name));
                    break;
                }
                record.updated_at = crate::util::now_iso();
            }
        }
        let attempted = selected
            .iter()
            .filter_map(|pubkey| {
                records
                    .iter()
                    .find(|record| record.pubkey.eq_ignore_ascii_case(pubkey))
                    .map(|record| (pubkey.clone(), record.clone()))
            })
            .collect();
        if let Err(error) = save_managed_agents(&app, &records) {
            append_error(
                &mut prepare_error,
                format!("Could not save the template update: {error}"),
            );
        }
        (
            selected,
            originals,
            attempted,
            original_relays,
            names,
            prepare_error,
            target_version,
        )
    };

    if let Some(error) = prepare_error {
        let restore_result = restore_original_records(&app, &originals, &attempted);
        let restart_errors = if restore_result.is_ok() {
            restart_original_pairs(&app, &original_relays).await
        } else {
            Vec::new()
        };
        let mut recovery_errors = Vec::new();
        if let Err(restore_error) = restore_result {
            recovery_errors.push(format!(
                "could not restore the original configuration: {restore_error}"
            ));
        }
        recovery_errors.extend(
            restart_errors.into_iter().map(|(pubkey, restart_error)| {
                format!("could not restart {pubkey}: {restart_error}")
            }),
        );
        if recovery_errors.is_empty() {
            return Err(error);
        }
        return Err(format!(
            "{error} Recovery also failed: {}",
            recovery_errors.join("; ")
        ));
    }

    let mut update_error: Option<String> = None;
    for pubkey in &selected {
        if let Some(relays) = original_relays.get(pubkey) {
            for relay in relays {
                let start_result = start_managed_agent_runtime_pair_lazy(
                    pubkey.clone(),
                    relay.clone(),
                    app.clone(),
                );
                if let Err(error) = start_result {
                    update_error = Some(error);
                    break;
                }
                if let Err(error) = wait_for_ready(&app, pubkey, relay).await {
                    update_error = Some(error);
                    break;
                }
            }
        }
        if update_error.is_some() {
            break;
        }
    }

    if let Some(error) = update_error {
        let cleanup_result = (|| -> Result<(Vec<String>, HashSet<String>), String> {
            let _transition = state
                .managed_agent_runtime_transition
                .lock()
                .map_err(|lock_error| lock_error.to_string())?;
            let _store = state
                .managed_agents_store_lock
                .lock()
                .map_err(|lock_error| lock_error.to_string())?;
            let mut records = load_managed_agents(&app)?;
            let mut runtimes = state
                .managed_agent_processes
                .lock()
                .map_err(|lock_error| lock_error.to_string())?;
            let mut cleanup_errors = Vec::new();
            let mut stopped = HashSet::new();
            for pubkey in &selected {
                let record = match find_managed_agent_mut(&mut records, pubkey) {
                    Ok(record) => record,
                    Err(stop_error) => {
                        cleanup_errors.push(stop_error);
                        continue;
                    }
                };
                match stop_managed_agent_process(&app, record, &mut runtimes) {
                    Ok(()) => {
                        stopped.insert(pubkey.clone());
                    }
                    Err(stop_error) => cleanup_errors.push(format!(
                        "Could not stop {} during rollback: {stop_error}",
                        record.name
                    )),
                }
            }
            if let Err(save_error) = save_managed_agents(&app, &records) {
                cleanup_errors.push(format!(
                    "Could not save stopped state during rollback: {save_error}"
                ));
            }
            Ok((cleanup_errors, stopped))
        })();
        let (mut recovery_failures, cleanup_stopped) = match cleanup_result {
            Ok(result) => result,
            Err(cleanup_error) => (
                vec![format!("Could not begin rollback cleanup: {cleanup_error}")],
                HashSet::new(),
            ),
        };

        let restore_result = restore_original_records(&app, &originals, &attempted);
        let relays_to_restart: BTreeMap<String, Vec<String>> = original_relays
            .iter()
            .filter(|(pubkey, _)| cleanup_stopped.contains(*pubkey))
            .map(|(pubkey, relays)| (pubkey.clone(), relays.clone()))
            .collect();
        let restart_errors = if restore_result.is_ok() {
            restart_original_pairs(&app, &relays_to_restart).await
        } else {
            Vec::new()
        };
        if let Err(restore_error) = restore_result {
            recovery_failures.push(format!(
                "Could not restore the original configuration: {restore_error}"
            ));
        }
        recovery_failures.extend(
            restart_errors.into_iter().map(|(pubkey, restart_error)| {
                format!("Could not restart {pubkey}: {restart_error}")
            }),
        );
        if !recovery_failures.is_empty() {
            return Err(format!(
                "{error} Rollback failed: {}",
                recovery_failures.join("; ")
            ));
        }
        let agents = selected
            .iter()
            .map(|pubkey| AgentTemplateUpdateResult {
                pubkey: pubkey.clone(),
                name: names.get(pubkey).cloned().unwrap_or_else(|| pubkey.clone()),
                outcome: AgentTemplateUpdateOutcome::RolledBack,
                error: Some(error.clone()),
            })
            .collect();
        return Ok(ApplyAgentTemplateUpdateResponse {
            persona_id: input.persona_id,
            version: target_version,
            rolled_back: true,
            agents,
        });
    }

    let agents = selected
        .iter()
        .map(|pubkey| {
            let running = original_relays
                .get(pubkey)
                .is_some_and(|relays| !relays.is_empty());
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
    Ok(ApplyAgentTemplateUpdateResponse {
        persona_id: input.persona_id,
        version: target_version,
        rolled_back: false,
        agents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persona(updated_at: &str) -> crate::managed_agents::AgentDefinition {
        crate::managed_agents::AgentDefinition {
            id: "analytics".to_string(),
            display_name: "Analytics".to_string(),
            avatar_url: None,
            system_prompt: "Analyze the data.".to_string(),
            runtime: Some("codex".to_string()),
            model: None,
            provider: None,
            name_pool: Vec::new(),
            is_builtin: false,
            is_active: true,
            shared: false,
            source_team: None,
            source_team_persona_slug: None,
            catalog_source: None,
            env_vars: BTreeMap::new(),
            respond_to: None,
            respond_to_allowlist: Vec::new(),
            parallelism: None,
            created_at: "2026-08-02T10:00:00.000000000Z".to_string(),
            updated_at: updated_at.to_string(),
        }
    }

    fn record() -> ManagedAgentRecord {
        let mut record = persona("2026-08-02T10:00:00.000000001Z").into_agent_record();
        record.pubkey = "a".repeat(64);
        record.persona_id = Some("analytics".to_string());
        record.pinned_persona_env_vars = Some(BTreeMap::new());
        record
    }

    #[test]
    fn selection_rejects_duplicates() {
        let selected = vec!["aa".repeat(32), "AA".repeat(32)];
        let error = validate_selection(&[], "analytics", &selected)
            .expect_err("duplicate selection should fail before lookup");
        assert!(error.contains("selected more than once"));
    }

    #[test]
    fn expected_version_check_rejects_a_newer_locked_persona_head() {
        let previewed = persona("2026-08-02T10:00:00.000000001Z");
        let edited = persona("2026-08-02T10:00:00.000000002Z");
        let expected = persona_version(&previewed);

        assert_eq!(
            checked_persona_version(&previewed, &expected).unwrap(),
            expected
        );
        assert!(
            checked_persona_version(&edited, &expected)
                .unwrap_err()
                .contains("changed while you were reviewing"),
            "the head reloaded under the store lock must be revalidated"
        );
    }

    #[test]
    fn preview_eligibility_blocks_remote_and_unready_local_agents() {
        let readiness_error = "Analytics needs an API key.".to_string();
        assert_eq!(
            template_update_blocked_reason(&BackendKind::Local, None),
            None
        );
        assert_eq!(
            template_update_blocked_reason(&BackendKind::Local, Some(readiness_error.clone())),
            Some(readiness_error)
        );
        assert_eq!(
            template_update_blocked_reason(
                &BackendKind::Provider {
                    id: "cloud".to_string(),
                    config: serde_json::Value::Null,
                },
                Some("ignored".to_string()),
            ),
            Some("Remote agents cannot be updated safely in this version.".to_string())
        );
    }

    #[test]
    fn readiness_formatter_is_actionable_without_rust_debug_syntax() {
        let text = format_readiness_requirements(&[
            crate::managed_agents::Requirement::NormalizedField {
                field: "provider".to_string(),
            },
            crate::managed_agents::Requirement::EnvKey {
                key: "OPENAI_COMPAT_API_KEY".to_string(),
            },
            crate::managed_agents::Requirement::MissingBinary {
                command: "pi".to_string(),
            },
        ]);
        assert_eq!(
            text,
            "choose a provider; set the required OPENAI_COMPAT_API_KEY environment variable; install pi or add it to PATH"
        );
        assert!(!text.contains("Requirement"));
        assert!(!text.contains('{'));
    }

    #[test]
    fn rollback_guard_rejects_concurrent_config_with_same_persona_version() {
        let original = record();
        let mut attempted = original.clone();
        attempted.system_prompt = Some("new prompt".to_string());
        attempted.persona_source_version = Some("new-version".to_string());
        let mut concurrent = attempted.clone();
        concurrent
            .env_vars
            .insert("INSTANCE_ONLY".to_string(), "new-value".to_string());

        assert!(rollback_guard_allows(&attempted, &original, &attempted));
        assert!(!rollback_guard_allows(&concurrent, &original, &attempted));
    }

    #[test]
    fn rollback_guard_accepts_atomic_save_original_and_preserves_runtime_state() {
        let original = record();
        let mut attempted = original.clone();
        attempted.system_prompt = Some("new prompt".to_string());
        assert!(rollback_guard_allows(&original, &original, &attempted));

        attempted.runtime_pid = Some(42);
        attempted.updated_at = "runtime-update".to_string();
        attempted.last_started_at = Some("started".to_string());
        assert!(same_nonvolatile_config(&attempted, &{
            let mut expected = attempted.clone();
            expected.runtime_pid = None;
            expected.updated_at.clear();
            expected.last_started_at = None;
            expected
        }));

        restore_config_preserving_runtime_state(&mut attempted, &original);
        assert_eq!(attempted.runtime_pid, Some(42));
        assert_eq!(attempted.updated_at, "runtime-update");
        assert_eq!(attempted.last_started_at.as_deref(), Some("started"));
        assert!(same_nonvolatile_config(&attempted, &original));
    }
}
