use super::*;

pub(super) fn checked_published_version(
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

pub(super) fn template_update_blocked_reason(
    backend: &BackendKind,
    readiness_error: Option<String>,
) -> Option<String> {
    if backend != &BackendKind::Local {
        return Some("Remote agents cannot be updated safely in this version.".to_string());
    }
    readiness_error
}

pub(super) fn tool_changes(
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

pub(super) fn skill_changes(
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

pub(super) fn instruction_change(
    record: &ManagedAgentRecord,
    target: &crate::managed_agents::AgentDefinition,
) -> Option<AgentTemplateInstructionChange> {
    let before = record.system_prompt.clone().unwrap_or_default();
    (before != target.system_prompt).then(|| AgentTemplateInstructionChange {
        before,
        after: target.system_prompt.clone(),
        // The preview only discloses that an override exists. Its contents
        // remain private to the individual agent.
        private_override_preserved: record.system_prompt_override.is_some(),
    })
}

pub(super) fn prospective_record(
    record: &ManagedAgentRecord,
    persona: &crate::managed_agents::AgentDefinition,
    bindings: BTreeMap<String, String>,
) -> Result<ManagedAgentRecord, String> {
    let mut prospective = record.clone();
    crate::managed_agents::persona_events::apply_persona_snapshot(&mut prospective, persona)?;
    prospective.connection_bindings = bindings;
    Ok(prospective)
}

pub(super) fn retained_bindings(
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

pub(super) fn validate_selection(
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
                instruction_change: instruction_change(record, &target),
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

pub(super) fn prospective_readiness(
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

pub(super) fn format_readiness_requirements(
    requirements: &[crate::managed_agents::Requirement],
) -> String {
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
                ManagedAgentRuntimeLifecycle::Listening | ManagedAgentRuntimeLifecycle::Ready => {
                    return Ok(());
                }
                ManagedAgentRuntimeLifecycle::Failed => {
                    return Err(runtime.error.clone().unwrap_or_else(|| {
                        "The updated agent failed its readiness check.".into()
                    }));
                }
                ManagedAgentRuntimeLifecycle::Starting
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
