use super::recovery::*;
use super::*;

pub(super) async fn apply_agent_template_update(
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
            materialize_template_runtime_defaults(&mut prospective, &global);
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
        for key in &original_keys {
            emit_agent_update_progress(
                &app,
                &input.request_id,
                &key.pubkey,
                AgentTemplateUpdateProgressStage::FinishingCurrentTask,
            );
        }
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
        emit_agent_update_progress(
            &app,
            &input.request_id,
            &key.pubkey,
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
                emit_agent_update_progress(
                    &app,
                    &input.request_id,
                    &key.pubkey,
                    AgentTemplateUpdateProgressStage::CheckingUpdate,
                );
                if let Err(error) = wait_for_ready(&app, &key.pubkey, &key.relay_url).await {
                    update_error = Some(format!(
                        "{} on {} did not become available: {error}",
                        key.pubkey, key.relay_url
                    ));
                    break;
                }
                emit_agent_update_progress(
                    &app,
                    &input.request_id,
                    &key.pubkey,
                    AgentTemplateUpdateProgressStage::Updated,
                );
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
    for pubkey in selected.iter().filter(|pubkey| {
        !original_keys
            .iter()
            .any(|key| key.pubkey.as_str() == pubkey.as_str())
    }) {
        emit_agent_update_progress(
            &app,
            &input.request_id,
            pubkey,
            AgentTemplateUpdateProgressStage::Updated,
        );
    }
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
