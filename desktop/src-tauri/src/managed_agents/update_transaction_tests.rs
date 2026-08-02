use std::{collections::BTreeMap, fs, time::Duration};

use tempfile::TempDir;

use super::*;

fn target() -> ImmutableUpdateTarget {
    ImmutableUpdateTarget {
        persona_id: "analytics".to_string(),
        version: AgentTemplateVersionRef {
            repo_address: format!("30617:{}:buzz-agent-templates", "a".repeat(64)),
            commit_oid: "b".repeat(40),
            artifact_path: format!(
                "templates/analytics/versions/{}/template.json",
                "c".repeat(64)
            ),
            artifact_sha256: "c".repeat(64),
        },
    }
}

fn record(pubkey: &str) -> ManagedAgentRecord {
    let mut record: ManagedAgentRecord = serde_json::from_str(&format!(
        r#"{{
            "pubkey": "{pubkey}",
            "name": "test",
            "persona_id": "analytics",
            "private_key_nsec": "",
            "relay_url": "wss://relay.example",
            "acp_command": "buzz-acp",
            "agent_command": "codex-acp",
            "agent_args": [],
            "mcp_command": "",
            "turn_timeout_seconds": 320,
            "system_prompt": null,
            "model": null,
            "provider": null,
            "env_vars": {{}},
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "last_started_at": null,
            "last_stopped_at": null,
            "last_exit_code": null,
            "last_error": null
        }}"#
    ))
    .expect("record fixture");
    record.persona_source_version = Some(target().version.authority_token().unwrap());
    record
}

fn generation(pubkey: &str, nonce_byte: char) -> JournalRuntimeGeneration {
    let start_nonce = nonce_byte.to_string().repeat(32);
    JournalRuntimeGeneration {
        key: ManagedAgentRuntimeKey::new(pubkey, "wss://relay.example").unwrap(),
        start_nonce: start_nonce.clone(),
        handoff: JournalHandoffIdentity {
            handoff_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            start_nonce,
        },
    }
}

fn transaction() -> UpdateTransaction {
    let pubkey = "d".repeat(64);
    let original = record(&pubkey);
    let mut attempted = original.clone();
    attempted.system_prompt = Some("updated".to_string());
    attempted.persona_source_version = Some(target().version.authority_token().unwrap());
    let selected = BTreeMap::from([(
        pubkey.clone(),
        SelectedAgentRecords {
            original,
            attempted,
        },
    )]);
    UpdateTransaction::new(
        uuid::Uuid::new_v4().hyphenated().to_string(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
        target(),
        selected,
        vec![generation(&pubkey, '1')],
    )
    .unwrap()
}

fn journal() -> (TempDir, UpdateTransactionJournal) {
    let temp = tempfile::tempdir().unwrap();
    let value = UpdateTransactionJournal::open_at(temp.path()).unwrap();
    (temp, value)
}

#[test]
fn create_load_list_update_and_delete_round_trip() {
    let (_temp, journal) = journal();
    let transaction = transaction();
    journal.create(&transaction).unwrap();
    assert!(journal.create(&transaction).is_err());
    assert!(journal.load(&transaction.transaction_id).unwrap() == transaction);
    assert!(journal.list().unwrap() == vec![transaction.clone()]);

    let before_handoff = journal
        .update(
            &transaction.transaction_id,
            0,
            UpdateTransactionStage::BeforeOriginalHandoff,
            Vec::new(),
        )
        .unwrap();
    assert_eq!(before_handoff.revision, 1);
    assert!(journal
        .update(
            &transaction.transaction_id,
            0,
            UpdateTransactionStage::AfterOriginalHandoff,
            Vec::new(),
        )
        .is_err());
    assert!(journal
        .delete(&transaction.transaction_id, before_handoff.revision)
        .is_err());

    let mut current = before_handoff;
    for stage in [
        UpdateTransactionStage::AfterOriginalHandoff,
        UpdateTransactionStage::BeforeCandidateCommit,
        UpdateTransactionStage::AfterCandidateCommit,
    ] {
        current = journal
            .update(
                &transaction.transaction_id,
                current.revision,
                stage,
                Vec::new(),
            )
            .unwrap();
    }
    let candidate = generation(&"d".repeat(64), '2');
    current = journal
        .update(
            &transaction.transaction_id,
            current.revision,
            UpdateTransactionStage::BeforeCandidateLaunch,
            vec![candidate],
        )
        .unwrap();
    current = journal
        .update(
            &transaction.transaction_id,
            current.revision,
            UpdateTransactionStage::AfterCandidateReady,
            current.candidate_generations.clone(),
        )
        .unwrap();
    current = journal
        .update(
            &transaction.transaction_id,
            current.revision,
            UpdateTransactionStage::UpdateCompleted,
            current.candidate_generations.clone(),
        )
        .unwrap();
    journal
        .delete(&transaction.transaction_id, current.revision)
        .unwrap();
    assert!(journal.list().unwrap().is_empty());
}

#[test]
fn every_crash_stage_reopens_with_a_conservative_recovery_decision() {
    for (index, stage) in UpdateTransactionStage::ALL.into_iter().enumerate() {
        let (temp, journal) = journal();
        let mut transaction = transaction();
        transaction.stage = stage;
        transaction.revision = index as u64;
        transaction.updated_at_unix_secs = transaction.created_at_unix_secs + index as u64;
        if matches!(
            stage,
            UpdateTransactionStage::BeforeCandidateLaunch
                | UpdateTransactionStage::AfterCandidateReady
                | UpdateTransactionStage::BeforeCandidateHandoff
                | UpdateTransactionStage::AfterCandidateHandoff
                | UpdateTransactionStage::BeforeOriginalRestore
                | UpdateTransactionStage::AfterOriginalRestore
                | UpdateTransactionStage::BeforeOriginalRestart
                | UpdateTransactionStage::AfterOriginalRestart
                | UpdateTransactionStage::UpdateCompleted
                | UpdateTransactionStage::RollbackCompleted
        ) {
            transaction.candidate_generations = vec![generation(&"d".repeat(64), '2')];
        }
        journal
            .write(
                &journal.path_for_id(&transaction.transaction_id).unwrap(),
                &transaction,
            )
            .unwrap();
        drop(journal);

        let reopened = UpdateTransactionJournal::open_at(temp.path()).unwrap();
        let entries = reopened.recovery_entries().unwrap();
        let [RecoveryJournalEntry::Transaction(recovered, decision)] = entries.as_slice() else {
            panic!("stage {stage:?} was not recovered");
        };
        assert_eq!(recovered.stage, stage);
        let expected = match stage {
            UpdateTransactionStage::Prepared => RecoveryDecision::DiscardUnstarted,
            UpdateTransactionStage::BeforeOriginalHandoff
            | UpdateTransactionStage::AfterOriginalHandoff
            | UpdateTransactionStage::BeforeCandidateCommit
            | UpdateTransactionStage::AfterCandidateCommit => RecoveryDecision::RestoreOriginals,
            UpdateTransactionStage::BeforeCandidateLaunch
            | UpdateTransactionStage::AfterCandidateReady
            | UpdateTransactionStage::BeforeCandidateHandoff => {
                RecoveryDecision::QuarantineCandidateMayHaveAcceptedWork
            }
            UpdateTransactionStage::AfterCandidateHandoff
            | UpdateTransactionStage::BeforeOriginalRestore => {
                RecoveryDecision::RestoreOriginalsAfterCandidateQuiesced
            }
            UpdateTransactionStage::AfterOriginalRestore
            | UpdateTransactionStage::BeforeOriginalRestart => RecoveryDecision::RestartOriginals,
            UpdateTransactionStage::AfterOriginalRestart => RecoveryDecision::FinishRollback,
            UpdateTransactionStage::UpdateCompleted | UpdateTransactionStage::RollbackCompleted => {
                RecoveryDecision::DeleteCompleted
            }
        };
        assert_eq!(*decision, expected, "wrong recovery at {stage:?}");
        if matches!(
            stage,
            UpdateTransactionStage::BeforeCandidateLaunch
                | UpdateTransactionStage::AfterCandidateReady
                | UpdateTransactionStage::BeforeCandidateHandoff
        ) {
            assert_eq!(
                *decision,
                RecoveryDecision::QuarantineCandidateMayHaveAcceptedWork,
                "stage {stage:?} must never automatically roll back a live or uncertain candidate"
            );
            assert!(!decision.allows_automatic_candidate_rollback());
        }
    }
}

#[test]
fn rollback_path_requires_candidate_quiescence_before_restore() {
    let (_temp, journal) = journal();
    let transaction = transaction();
    let id = transaction.transaction_id.clone();
    journal.create(&transaction).unwrap();
    let mut current = transaction;
    for stage in [
        UpdateTransactionStage::BeforeOriginalHandoff,
        UpdateTransactionStage::AfterOriginalHandoff,
        UpdateTransactionStage::BeforeCandidateCommit,
        UpdateTransactionStage::AfterCandidateCommit,
    ] {
        current = journal
            .update(&id, current.revision, stage, Vec::new())
            .unwrap();
    }
    let candidates = vec![generation(&"d".repeat(64), '2')];
    current = journal
        .update(
            &id,
            current.revision,
            UpdateTransactionStage::BeforeCandidateLaunch,
            candidates.clone(),
        )
        .unwrap();
    assert_eq!(
        current.recovery_decision(),
        RecoveryDecision::QuarantineCandidateMayHaveAcceptedWork
    );
    current = journal
        .update(
            &id,
            current.revision,
            UpdateTransactionStage::BeforeCandidateHandoff,
            candidates.clone(),
        )
        .unwrap();
    assert_eq!(
        current.recovery_decision(),
        RecoveryDecision::QuarantineCandidateMayHaveAcceptedWork
    );
    current = journal
        .update(
            &id,
            current.revision,
            UpdateTransactionStage::AfterCandidateHandoff,
            candidates.clone(),
        )
        .unwrap();
    assert_eq!(
        current.recovery_decision(),
        RecoveryDecision::RestoreOriginalsAfterCandidateQuiesced
    );
    for stage in [
        UpdateTransactionStage::BeforeOriginalRestore,
        UpdateTransactionStage::AfterOriginalRestore,
        UpdateTransactionStage::BeforeOriginalRestart,
        UpdateTransactionStage::AfterOriginalRestart,
        UpdateTransactionStage::RollbackCompleted,
    ] {
        current = journal
            .update(&id, current.revision, stage, candidates.clone())
            .unwrap();
    }
    assert_eq!(
        current.recovery_decision(),
        RecoveryDecision::DeleteCompleted
    );
    journal.delete(&id, current.revision).unwrap();
}

#[test]
fn pre_candidate_failures_can_finish_rollback_without_inventing_a_candidate() {
    for path in [
        vec![
            UpdateTransactionStage::BeforeOriginalHandoff,
            UpdateTransactionStage::BeforeOriginalRestart,
            UpdateTransactionStage::AfterOriginalRestart,
            UpdateTransactionStage::RollbackCompleted,
        ],
        vec![
            UpdateTransactionStage::BeforeOriginalHandoff,
            UpdateTransactionStage::AfterOriginalHandoff,
            UpdateTransactionStage::BeforeCandidateCommit,
            UpdateTransactionStage::BeforeOriginalRestore,
            UpdateTransactionStage::AfterOriginalRestore,
            UpdateTransactionStage::BeforeOriginalRestart,
            UpdateTransactionStage::AfterOriginalRestart,
            UpdateTransactionStage::RollbackCompleted,
        ],
    ] {
        let (_temp, journal) = journal();
        let mut current = transaction();
        let id = current.transaction_id.clone();
        journal.create(&current).unwrap();
        for stage in path {
            current = journal
                .update(&id, current.revision, stage, Vec::new())
                .unwrap();
        }
        assert!(current.candidate_generations.is_empty());
        assert_eq!(
            current.recovery_decision(),
            RecoveryDecision::DeleteCompleted
        );
        journal.delete(&id, current.revision).unwrap();
    }
}

#[test]
fn candidate_plan_is_immutable_after_the_launch_boundary() {
    let (_temp, journal) = journal();
    let mut current = transaction();
    let id = current.transaction_id.clone();
    journal.create(&current).unwrap();
    for stage in [
        UpdateTransactionStage::BeforeOriginalHandoff,
        UpdateTransactionStage::AfterOriginalHandoff,
        UpdateTransactionStage::BeforeCandidateCommit,
        UpdateTransactionStage::AfterCandidateCommit,
    ] {
        current = journal
            .update(&id, current.revision, stage, Vec::new())
            .unwrap();
    }
    let candidates = vec![generation(&"d".repeat(64), '2')];
    current = journal
        .update(
            &id,
            current.revision,
            UpdateTransactionStage::BeforeCandidateLaunch,
            candidates.clone(),
        )
        .unwrap();

    let mut changed = candidates;
    changed[0].start_nonce = "3".repeat(32);
    changed[0].handoff.start_nonce = "3".repeat(32);
    assert!(journal
        .update(
            &id,
            current.revision,
            UpdateTransactionStage::AfterCandidateReady,
            changed,
        )
        .is_err());
    assert_eq!(
        journal.load(&id).unwrap().stage,
        UpdateTransactionStage::BeforeCandidateLaunch
    );
}

#[test]
fn immutable_target_snapshots_and_generations_reject_tampering() {
    let (_temp, journal) = journal();
    let transaction = transaction();
    journal.create(&transaction).unwrap();
    let path = journal.path_for_id(&transaction.transaction_id).unwrap();
    let mut wire: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

    wire["target"]["version"]["artifactPath"] = "../escape".into();
    atomic_write_owner_file(&path, &serde_json::to_vec(&wire).unwrap()).unwrap();
    assert!(journal.load(&transaction.transaction_id).is_err());

    journal.delete_invalid_for_test(&path);
    journal.create(&transaction).unwrap();
    let mut changed = transaction.clone();
    changed.target.persona_id = "different".to_string();
    changed.revision = 1;
    changed.stage = UpdateTransactionStage::BeforeOriginalHandoff;
    assert!(changed.validate_update_from(&transaction).is_err());

    let mut mismatch = transaction.clone();
    mismatch.original_generations[0].handoff.start_nonce = "3".repeat(32);
    assert!(mismatch.validate().is_err());

    let mut missing_candidate = transaction.clone();
    missing_candidate.stage = UpdateTransactionStage::BeforeCandidateLaunch;
    assert!(missing_candidate.validate().is_err());

    let mut reused_generation = transaction.clone();
    reused_generation.stage = UpdateTransactionStage::BeforeCandidateLaunch;
    reused_generation.candidate_generations = reused_generation.original_generations.clone();
    assert!(reused_generation.validate().is_err());

    let mut duplicate_handoff = transaction.clone();
    duplicate_handoff.stage = UpdateTransactionStage::BeforeCandidateLaunch;
    let mut candidate = generation(&"d".repeat(64), '2');
    candidate.handoff.handoff_id = duplicate_handoff.original_generations[0]
        .handoff
        .handoff_id
        .clone();
    duplicate_handoff.candidate_generations = vec![candidate];
    assert!(duplicate_handoff.validate().is_err());

    let mut credential_change = transaction.clone();
    credential_change
        .selected
        .get_mut(&"d".repeat(64))
        .unwrap()
        .attempted
        .private_key_nsec = "changed".to_string();
    assert!(credential_change.validate().is_err());

    let mut exhausted = transaction;
    exhausted.revision = u64::MAX;
    assert!(exhausted.validate().is_err());
}

#[cfg(unix)]
#[test]
fn files_are_owner_only_and_links_are_quarantined_without_following_targets() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    let (temp, journal) = journal();
    let transaction = transaction();
    journal.create(&transaction).unwrap();
    let path = journal.path_for_id(&transaction.transaction_id).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    journal.delete(&transaction.transaction_id, 0).unwrap();
    let target = temp.path().join("outside");
    fs::write(&target, b"do not touch").unwrap();
    let name = format!("{}.json", transaction.transaction_id);
    symlink(&target, journal.active_dir.join(&name)).unwrap();
    let entries = journal.recovery_entries().unwrap();
    assert!(matches!(
        entries.as_slice(),
        [RecoveryJournalEntry::Quarantine(
            QuarantinedJournal {
                reason: QuarantineReason::LinkOrNonFile,
                ..
            },
            RecoveryDecision::QuarantineInvalidJournal
        )]
    ));
    let quarantined = journal.quarantine(&name).unwrap();
    assert!(quarantined
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&target).unwrap(), b"do not touch");
}

#[cfg(unix)]
#[test]
fn permissive_corrupt_and_oversized_documents_are_quarantine_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let (_temp, journal) = journal();
    let bad_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let bad_path = journal.path_for_id(&bad_id).unwrap();
    fs::write(&bad_path, b"{}").unwrap();
    fs::set_permissions(&bad_path, fs::Permissions::from_mode(0o644)).unwrap();
    let entries = journal.recovery_entries().unwrap();
    assert!(matches!(
        entries.as_slice(),
        [RecoveryJournalEntry::Quarantine(
            QuarantinedJournal {
                reason: QuarantineReason::InvalidPermissions,
                ..
            },
            _
        )]
    ));
    journal
        .quarantine(bad_path.file_name().unwrap().to_str().unwrap())
        .unwrap();

    let huge_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let huge_path = journal.path_for_id(&huge_id).unwrap();
    let huge = File::create(&huge_path).unwrap();
    huge.set_len(JOURNAL_MAX_BYTES + 1).unwrap();
    drop(huge);
    fs::set_permissions(&huge_path, fs::Permissions::from_mode(0o600)).unwrap();
    let entries = journal.recovery_entries().unwrap();
    assert!(matches!(
        entries.as_slice(),
        [RecoveryJournalEntry::Quarantine(
            QuarantinedJournal {
                reason: QuarantineReason::Oversized,
                ..
            },
            _
        )]
    ));
}

#[test]
fn strict_documents_reject_unknown_fields_and_filename_mismatches() {
    let (_temp, journal) = journal();
    let transaction = transaction();
    let mut wire = serde_json::to_value(&transaction).unwrap();
    wire["unexpected"] = true.into();
    let path = journal.path_for_id(&transaction.transaction_id).unwrap();
    atomic_write_owner_file(&path, &serde_json::to_vec(&wire).unwrap()).unwrap();
    assert!(journal.load(&transaction.transaction_id).is_err());

    let wrong_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let wrong_path = journal.path_for_id(&wrong_id).unwrap();
    atomic_write_owner_file(&wrong_path, &serde_json::to_vec(&transaction).unwrap()).unwrap();
    assert!(journal.load(&wrong_id).is_err());
}

#[test]
fn crash_leftover_atomic_write_is_quarantined_without_hiding_the_stable_stage() {
    let (_temp, journal) = journal();
    let transaction = transaction();
    journal.create(&transaction).unwrap();
    let leftover_name = ".create-interrupted.tmp";
    let leftover = journal.active_dir.join(leftover_name);
    write_new_owner_file(&leftover, b"incomplete").unwrap();

    let entries = journal.recovery_entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| matches!(
        entry,
        RecoveryJournalEntry::Transaction(recovered, RecoveryDecision::DiscardUnstarted)
            if recovered.transaction_id == transaction.transaction_id
    )));
    assert!(entries.iter().any(|entry| matches!(
        entry,
        RecoveryJournalEntry::Quarantine(
            QuarantinedJournal {
                source_name,
                reason: QuarantineReason::InvalidName,
            },
            RecoveryDecision::QuarantineInvalidJournal
        ) if source_name == leftover_name
    )));
    journal.quarantine(leftover_name).unwrap();
    assert_eq!(journal.recovery_entries().unwrap().len(), 1);
}

#[test]
fn timestamps_and_selection_are_bounded() {
    let mut future = transaction();
    future.updated_at_unix_secs =
        unix_now_secs() + JOURNAL_MAX_FUTURE_SKEW_SECS + Duration::from_secs(1).as_secs();
    assert!(future.validate().is_err());

    let mut empty = transaction();
    empty.selected.clear();
    assert!(empty.validate().is_err());
}

impl UpdateTransactionJournal {
    fn delete_invalid_for_test(&self, path: &Path) {
        fs::remove_file(path).unwrap();
        sync_directory(&self.active_dir).unwrap();
    }
}
