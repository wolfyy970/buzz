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
        stop_managed_agent_process, AgentReadiness, AgentTemplateVersionRef, BackendKind,
        ManagedAgentRecord, ManagedAgentRuntimeKey, ManagedAgentRuntimeLifecycle,
    },
};

use super::agent_template_versions::load_agent_template_version;

const UPDATE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_READY_POLL: Duration = Duration::from_millis(200);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAgentTemplateUpdateRequest {
    pub persona_id: String,
    pub expected_version: AgentTemplateVersionRef,
    pub selected_pubkeys: Vec<String>,
    #[serde(default)]
    pub connection_bindings_by_pubkey: BTreeMap<String, BTreeMap<String, String>>,
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
    pub version: AgentTemplateVersionRef,
    pub rolled_back: bool,
    pub agents: Vec<AgentTemplateUpdateResult>,
}

fn checked_published_version(
    persona: &crate::managed_agents::AgentDefinition,
    expected_version: &AgentTemplateVersionRef,
) -> Result<AgentTemplateVersionRef, String> {
    let current_version = persona
        .published_version
        .as_ref()
        .ok_or_else(|| "Publish a template version before updating agents.".to_string())?;
    if current_version != expected_version {
        return Err(
            "A newer template version was published while you were reviewing it. Review the affected agents again."
                .to_string(),
        );
    }
    current_version.validate()?;
    Ok(current_version.clone())
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

fn tool_changes(
    before: &[crate::managed_agents::AgentToolRequirement],
    after: &[crate::managed_agents::AgentToolRequirement],
) -> AgentTemplateToolChanges {
    let before_by_id: BTreeMap<_, _> = before
        .iter()
        .map(|requirement| (requirement.id.as_str(), requirement))
        .collect();
    let after_by_id: BTreeMap<_, _> = after
        .iter()
        .map(|requirement| (requirement.id.as_str(), requirement))
        .collect();
    let added = after
        .iter()
        .filter(|requirement| !before_by_id.contains_key(requirement.id.as_str()))
        .cloned()
        .collect();
    let changed = after
        .iter()
        .filter_map(|requirement| {
            let previous = before_by_id.get(requirement.id.as_str())?;
            (*previous != requirement).then(|| AgentTemplateToolRequirementChange {
                before: (*previous).clone(),
                after: requirement.clone(),
            })
        })
        .collect();
    let removed = before
        .iter()
        .filter(|requirement| !after_by_id.contains_key(requirement.id.as_str()))
        .cloned()
        .collect();
    AgentTemplateToolChanges {
        added,
        changed,
        removed,
    }
}

fn skill_changes(
    before: &[crate::managed_agents::AgentSkill],
    after: &[crate::managed_agents::AgentSkill],
) -> AgentTemplateSkillChanges {
    let before_by_name: BTreeMap<_, _> = before
        .iter()
        .map(|skill| (skill.name.as_str(), skill))
        .collect();
    let after_by_name: BTreeMap<_, _> = after
        .iter()
        .map(|skill| (skill.name.as_str(), skill))
        .collect();
    AgentTemplateSkillChanges {
        added: after
            .iter()
            .filter(|skill| !before_by_name.contains_key(skill.name.as_str()))
            .cloned()
            .collect(),
        changed: after
            .iter()
            .filter_map(|skill| {
                let previous = before_by_name.get(skill.name.as_str())?;
                (*previous != skill).then(|| AgentTemplateSkillChange {
                    before: (*previous).clone(),
                    after: skill.clone(),
                })
            })
            .collect(),
        removed: before
            .iter()
            .filter(|skill| !after_by_name.contains_key(skill.name.as_str()))
            .cloned()
            .collect(),
    }
}

fn prospective_record(
    record: &ManagedAgentRecord,
    persona: &crate::managed_agents::AgentDefinition,
    bindings: BTreeMap<String, String>,
) -> Result<ManagedAgentRecord, String> {
    let mut prospective = record.clone();
    crate::managed_agents::persona_events::apply_persona_snapshot(&mut prospective, persona)?;
    prospective.connection_bindings = bindings;
    Ok(prospective)
}

fn retained_bindings(
    record: &ManagedAgentRecord,
    persona: &crate::managed_agents::AgentDefinition,
) -> BTreeMap<String, String> {
    let target_ids: HashSet<&str> = persona
        .tool_requirements
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect();
    record
        .connection_bindings
        .iter()
        .filter(|(id, _)| target_ids.contains(id.as_str()))
        .map(|(id, connection)| (id.clone(), connection.clone()))
        .collect()
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
pub async fn preview_agent_template_update(
    persona_id: String,
    target_version: Option<AgentTemplateVersionRef>,
    app: AppHandle,
) -> Result<AgentTemplateUpdatePreview, String> {
    let state = app.state::<AppState>();
    let (records, mut personas, persona, target_version) = {
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let records = load_managed_agents(&app)?;
        let personas = load_personas(&app)?;
        let persona = personas
            .iter()
            .find(|persona| persona.id == persona_id)
            .cloned()
            .ok_or_else(|| format!("Template {persona_id} no longer exists."))?;
        let published = persona
            .published_version
            .clone()
            .ok_or_else(|| "Publish a template version before updating agents.".to_string())?;
        if target_version
            .as_ref()
            .is_some_and(|requested| requested != &published)
        {
            return Err(
                "A newer template version was published. Open the update review again.".to_string(),
            );
        }
        (records, personas, persona, published)
    };
    let target_version_token = target_version.authority_token()?;
    let load_app = app.clone();
    let load_persona = persona.clone();
    let load_version = target_version.clone();
    let target = tokio::task::spawn_blocking(move || {
        load_agent_template_version(&load_app, &load_persona, &load_version)
    })
    .await
    .map_err(|error| format!("Template version task failed: {error}"))??;
    if let Some(slot) = personas.iter_mut().find(|item| item.id == persona_id) {
        *slot = target.clone();
    }
    let global = load_global_agent_config(&app).unwrap_or_default();
    let preview_rows: Vec<(&ManagedAgentRecord, Option<String>)> = records
        .iter()
        .filter(|record| record.persona_id.as_deref() == Some(persona_id.as_str()))
        .map(|record| {
            let readiness_error = (record.backend == BackendKind::Local)
                .then(|| prospective_readiness(record, &target, &personas, &global).err())
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
        .map(|(record, blocked_reason)| {
            let bindings = retained_bindings(record, &target);
            let tool_binding_issues = prospective_record(record, &target, bindings.clone())
                .and_then(|prospective| {
                    crate::managed_agents::project_connections::agent_tool_binding_issues(
                        &app,
                        &prospective,
                    )
                })
                .map(|issues| {
                    issues
                        .into_iter()
                        .map(|issue| issue.reason)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|error| vec![error]);
            AgentTemplateUpdateTarget {
                pubkey: record.pubkey.clone(),
                name: record.name.clone(),
                current_version: record.persona_source_version.clone(),
                target_version: target_version.clone(),
                running_relays: running_relays_for(&runtimes, &record.pubkey),
                eligible: blocked_reason.is_none(),
                blocked_reason,
                project_scope: record.project_scope.clone(),
                connection_bindings: bindings,
                tool_changes: tool_changes(
                    &record.pinned_tool_requirements,
                    &target.tool_requirements,
                ),
                skill_changes: skill_changes(&record.pinned_skills, &target.skills),
                tool_binding_issues,
            }
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
        target_version_token,
        target_tool_requirements: target.tool_requirements,
        target_skills: target.skills,
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

pub(crate) async fn wait_for_ready(
    app: &AppHandle,
    pubkey: &str,
    relay_url: &str,
) -> Result<(), String> {
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

    let (selected, originals, attempted, original_relays, names, prepare_error) = {
        let _transition = state
            .managed_agent_runtime_transition
            .lock()
            .map_err(|error| error.to_string())?;
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        // Recheck the published reference under the update transaction. The
        // artifact was already loaded and verified outside these locks, so a
        // concurrent publish rejects the update without substituting a newer
        // mutable head or holding a lock across Git I/O.
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
        let mut records = load_managed_agents(&app)?;
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

        let mut staged_bindings = BTreeMap::new();
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
            let prospective = prospective_record(record, &target_persona, bindings.clone())?;
            crate::managed_agents::project_connections::validate_agent_project_connections(
                &app,
                &prospective,
            )?;
            staged_bindings.insert(pubkey.clone(), bindings);
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
                    record,
                    &target_persona,
                ) {
                    prepare_error = Some(format!("Could not update {}: {error}", record.name));
                    break;
                }
                record.persona_source_version = Some(target_version_token.clone());
                record.connection_bindings =
                    staged_bindings.get(pubkey).cloned().unwrap_or_default();
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
#[path = "agent_template_updates/tests.rs"]
mod tests;
