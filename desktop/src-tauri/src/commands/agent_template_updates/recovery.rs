use super::*;

pub(super) fn list_agent_template_update_recoveries(
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
pub(super) async fn restore_interrupted_agent_template_update(
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

pub(super) fn restore_original_records(
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

pub(super) fn normalize_runtime_state(record: &mut ManagedAgentRecord) {
    record.runtime_pid = None;
    record.updated_at.clear();
    record.last_started_at = None;
    record.last_stopped_at = None;
    record.last_exit_code = None;
    record.last_error = None;
    record.last_error_code = None;
}

pub(super) fn same_nonvolatile_config(
    current: &ManagedAgentRecord,
    expected: &ManagedAgentRecord,
) -> bool {
    let mut current = current.clone();
    let mut expected = expected.clone();
    normalize_runtime_state(&mut current);
    normalize_runtime_state(&mut expected);
    current == expected
}

pub(super) fn rollback_guard_allows(
    current: &ManagedAgentRecord,
    original: &ManagedAgentRecord,
    attempted: &ManagedAgentRecord,
) -> bool {
    same_nonvolatile_config(current, attempted) || same_nonvolatile_config(current, original)
}

pub(super) fn restore_config_preserving_runtime_state(
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

pub(super) fn runtime_keys_for_selected(
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

pub(super) async fn drain_claimed_runtime_keys(
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

pub(super) fn clear_rollback_safe_claims(
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

pub(super) fn clear_runtime_claims(
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

pub(super) fn absent_drained_runtime_keys(
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
pub(super) enum DrainFailureState {
    OriginalStillRunning,
    ExitedWithoutCheckpoint,
    OwnershipUncertain,
}

pub(super) fn classify_drain_failure(
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

pub(super) fn rollback_safe_failure_lost_runtime(
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

pub(super) async fn restart_runtime_keys(
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

pub(super) fn failure_response(
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

pub(super) fn describe_drain_errors(
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

pub(super) fn advance_update_transaction(
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

pub(super) fn retain_interrupted_update_block(
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

pub(super) fn cleanup_unstarted_update_journal(
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
