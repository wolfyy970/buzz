//! Durable, private journal for managed-agent template update transactions.
//!
//! The journal is the startup authority after a Desktop crash. Each transition
//! is persisted before and after an irreversible boundary so recovery can
//! choose conservatively. In particular, no stage at which a candidate may
//! have accepted work is ever classified for automatic rollback.

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::fs::{self, File};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _};

use super::{
    managed_agents_base_dir, AgentTemplateVersionRef, ManagedAgentRecord, ManagedAgentRuntimeKey,
};

const JOURNAL_VERSION: u32 = 1;
const JOURNAL_KIND: &str = "buzz-agent-template-update";
const JOURNAL_DIR: &str = "update-transactions";
const ACTIVE_DIR: &str = "active";
const QUARANTINE_DIR: &str = "quarantine";
const JOURNAL_MAX_BYTES: u64 = 8 * 1024 * 1024;
const JOURNAL_MAX_SELECTED: usize = 256;
const JOURNAL_MAX_GENERATIONS: usize = 1_024;
const JOURNAL_MAX_FUTURE_SKEW_SECS: u64 = 300;

#[path = "update_transaction_fs.rs"]
mod fs_support;
use fs_support::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImmutableUpdateTarget {
    pub(crate) persona_id: String,
    pub(crate) version: AgentTemplateVersionRef,
}

// Deliberately no `Debug`: managed records can contain local credential
// fallback material and must never become loggable through the journal.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SelectedAgentRecords {
    pub(crate) original: ManagedAgentRecord,
    pub(crate) attempted: ManagedAgentRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JournalHandoffIdentity {
    pub(crate) handoff_id: String,
    pub(crate) start_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JournalRuntimeGeneration {
    pub(crate) key: ManagedAgentRuntimeKey,
    pub(crate) start_nonce: String,
    pub(crate) handoff: JournalHandoffIdentity,
}

/// Durable checkpoints around every update boundary that can change ownership
/// of work or the selected agent configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateTransactionStage {
    Prepared,
    BeforeOriginalHandoff,
    AfterOriginalHandoff,
    BeforeCandidateCommit,
    AfterCandidateCommit,
    BeforeCandidateLaunch,
    AfterCandidateReady,
    BeforeCandidateHandoff,
    AfterCandidateHandoff,
    BeforeOriginalRestore,
    AfterOriginalRestore,
    BeforeOriginalRestart,
    AfterOriginalRestart,
    UpdateCompleted,
    RollbackCompleted,
}

impl UpdateTransactionStage {
    #[cfg(test)]
    const ALL: [Self; 15] = [
        Self::Prepared,
        Self::BeforeOriginalHandoff,
        Self::AfterOriginalHandoff,
        Self::BeforeCandidateCommit,
        Self::AfterCandidateCommit,
        Self::BeforeCandidateLaunch,
        Self::AfterCandidateReady,
        Self::BeforeCandidateHandoff,
        Self::AfterCandidateHandoff,
        Self::BeforeOriginalRestore,
        Self::AfterOriginalRestore,
        Self::BeforeOriginalRestart,
        Self::AfterOriginalRestart,
        Self::UpdateCompleted,
        Self::RollbackCompleted,
    ];

    fn can_transition_to(self, next: Self) -> bool {
        use UpdateTransactionStage as S;
        matches!(
            (self, next),
            (S::Prepared, S::BeforeOriginalHandoff)
                | (S::BeforeOriginalHandoff, S::AfterOriginalHandoff)
                | (S::AfterOriginalHandoff, S::BeforeCandidateCommit)
                | (S::BeforeCandidateCommit, S::AfterCandidateCommit)
                | (S::AfterCandidateCommit, S::BeforeCandidateLaunch)
                | (S::BeforeCandidateLaunch, S::AfterCandidateReady)
                | (S::AfterCandidateReady, S::UpdateCompleted)
                | (S::BeforeCandidateLaunch, S::BeforeCandidateHandoff)
                | (S::AfterCandidateReady, S::BeforeCandidateHandoff)
                | (S::BeforeCandidateHandoff, S::AfterCandidateHandoff)
                | (S::BeforeOriginalHandoff, S::BeforeOriginalRestart)
                | (S::AfterOriginalHandoff, S::BeforeOriginalRestart)
                | (S::BeforeCandidateCommit, S::BeforeOriginalRestore)
                | (S::AfterCandidateCommit, S::BeforeOriginalRestore)
                | (S::AfterCandidateHandoff, S::BeforeOriginalRestore)
                | (S::BeforeOriginalRestore, S::AfterOriginalRestore)
                | (S::AfterOriginalRestore, S::BeforeOriginalRestart)
                | (S::BeforeOriginalRestart, S::AfterOriginalRestart)
                | (S::AfterOriginalRestart, S::RollbackCompleted)
        )
    }
}

// Deliberately no `Debug`: selected snapshots can contain local credentials.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdateTransaction {
    version: u32,
    kind: String,
    pub(crate) transaction_id: String,
    pub(crate) request_id: String,
    pub(crate) revision: u64,
    pub(crate) created_at_unix_secs: u64,
    pub(crate) updated_at_unix_secs: u64,
    pub(crate) stage: UpdateTransactionStage,
    pub(crate) target: ImmutableUpdateTarget,
    pub(crate) selected: BTreeMap<String, SelectedAgentRecords>,
    pub(crate) original_generations: Vec<JournalRuntimeGeneration>,
    pub(crate) candidate_generations: Vec<JournalRuntimeGeneration>,
}

impl UpdateTransaction {
    pub(crate) fn new(
        transaction_id: String,
        request_id: String,
        target: ImmutableUpdateTarget,
        selected: BTreeMap<String, SelectedAgentRecords>,
        original_generations: Vec<JournalRuntimeGeneration>,
    ) -> Result<Self, String> {
        let now = unix_now_secs();
        let value = Self {
            version: JOURNAL_VERSION,
            kind: JOURNAL_KIND.to_string(),
            transaction_id,
            request_id,
            revision: 0,
            created_at_unix_secs: now,
            updated_at_unix_secs: now,
            stage: UpdateTransactionStage::Prepared,
            target,
            selected,
            original_generations,
            candidate_generations: Vec::new(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != JOURNAL_VERSION || self.kind != JOURNAL_KIND {
            return Err("The update transaction has an unsupported format.".to_string());
        }
        if self.revision == u64::MAX {
            return Err("The update transaction revision is invalid.".to_string());
        }
        validate_canonical_uuid(&self.transaction_id, "transaction id")?;
        validate_canonical_uuid(&self.request_id, "request id")?;
        if self.created_at_unix_secs == 0
            || self.updated_at_unix_secs < self.created_at_unix_secs
            || self.updated_at_unix_secs
                > unix_now_secs().saturating_add(JOURNAL_MAX_FUTURE_SKEW_SECS)
        {
            return Err("The update transaction has invalid timestamps.".to_string());
        }
        validate_safe_id(&self.target.persona_id, "template id")?;
        self.target.version.validate()?;
        if !self
            .target
            .version
            .artifact_path
            .starts_with(&format!("templates/{}/versions/", self.target.persona_id))
        {
            return Err(
                "The immutable template version does not match the selected template.".to_string(),
            );
        }
        if self.selected.is_empty() || self.selected.len() > JOURNAL_MAX_SELECTED {
            return Err("The update transaction has an invalid agent selection.".to_string());
        }
        let target_token = self.target.version.authority_token()?;
        for (pubkey, records) in &self.selected {
            validate_pubkey(pubkey)?;
            if records.original.pubkey != *pubkey || records.attempted.pubkey != *pubkey {
                return Err(
                    "The update transaction record snapshots do not match their agent.".to_string(),
                );
            }
            if records.original.private_key_nsec != records.attempted.private_key_nsec
                || records.original.auth_tag != records.attempted.auth_tag
            {
                return Err(
                    "An update transaction cannot change an agent identity credential.".to_string(),
                );
            }
            if records.original.persona_id.as_deref() != Some(self.target.persona_id.as_str())
                || records.attempted.persona_id.as_deref() != Some(self.target.persona_id.as_str())
                || records.attempted.persona_source_version.as_deref() != Some(&target_token)
            {
                return Err(
                    "The update transaction snapshots do not match the immutable target."
                        .to_string(),
                );
            }
        }
        validate_generations(
            &self.original_generations,
            &self.selected,
            "original runtime",
        )?;
        validate_generations(
            &self.candidate_generations,
            &self.selected,
            "candidate runtime",
        )?;
        if stage_requires_candidate_plan(self.stage)
            && generation_keys(&self.original_generations)
                != generation_keys(&self.candidate_generations)
        {
            return Err(
                "The candidate runtime plan does not cover every original runtime pair."
                    .to_string(),
            );
        }
        for candidate in &self.candidate_generations {
            if self.original_generations.iter().any(|original| {
                original.key == candidate.key && original.start_nonce == candidate.start_nonce
            }) {
                return Err(
                    "A candidate runtime must use a new process generation identity.".to_string(),
                );
            }
        }
        let mut handoff_ids = HashSet::new();
        if self
            .original_generations
            .iter()
            .chain(&self.candidate_generations)
            .any(|generation| !handoff_ids.insert(&generation.handoff.handoff_id))
        {
            return Err("Update handoff identities must be unique.".to_string());
        }
        Ok(())
    }

    fn validate_update_from(&self, previous: &Self) -> Result<(), String> {
        self.validate()?;
        if self.transaction_id != previous.transaction_id
            || self.request_id != previous.request_id
            || self.created_at_unix_secs != previous.created_at_unix_secs
            || self.target != previous.target
            || self.selected != previous.selected
            || self.original_generations != previous.original_generations
        {
            return Err(
                "Immutable update transaction data changed during a journal update.".to_string(),
            );
        }
        if previous.revision.checked_add(1) != Some(self.revision)
            || self.updated_at_unix_secs < previous.updated_at_unix_secs
            || !previous.stage.can_transition_to(self.stage)
        {
            return Err("The update transaction stage or revision is invalid.".to_string());
        }
        if !candidate_generations_extend(
            &previous.candidate_generations,
            &self.candidate_generations,
        ) {
            return Err(
                "Candidate runtime generations may only be appended before launch.".to_string(),
            );
        }
        if self.candidate_generations != previous.candidate_generations
            && self.stage != UpdateTransactionStage::BeforeCandidateLaunch
        {
            return Err(
                "Candidate runtime generations must be made durable before launch.".to_string(),
            );
        }
        Ok(())
    }

    pub(crate) fn recovery_decision(&self) -> RecoveryDecision {
        use RecoveryDecision as D;
        use UpdateTransactionStage as S;
        match self.stage {
            S::Prepared => D::DiscardUnstarted,
            S::BeforeOriginalHandoff
            | S::AfterOriginalHandoff
            | S::BeforeCandidateCommit
            | S::AfterCandidateCommit => D::RestoreOriginals,
            S::BeforeCandidateLaunch | S::AfterCandidateReady | S::BeforeCandidateHandoff => {
                D::QuarantineCandidateMayHaveAcceptedWork
            }
            S::AfterCandidateHandoff | S::BeforeOriginalRestore => {
                D::RestoreOriginalsAfterCandidateQuiesced
            }
            S::AfterOriginalRestore | S::BeforeOriginalRestart => D::RestartOriginals,
            S::AfterOriginalRestart => D::FinishRollback,
            S::UpdateCompleted | S::RollbackCompleted => D::DeleteCompleted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryDecision {
    DiscardUnstarted,
    RestoreOriginals,
    QuarantineCandidateMayHaveAcceptedWork,
    RestoreOriginalsAfterCandidateQuiesced,
    RestartOriginals,
    FinishRollback,
    DeleteCompleted,
    QuarantineInvalidJournal,
}

impl RecoveryDecision {
    #[cfg(test)]
    pub(crate) fn allows_automatic_candidate_rollback(self) -> bool {
        matches!(
            self,
            Self::RestoreOriginals | Self::RestoreOriginalsAfterCandidateQuiesced
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::DiscardUnstarted => "discard_unstarted",
            Self::RestoreOriginals => "restore_originals",
            Self::QuarantineCandidateMayHaveAcceptedWork => "candidate_may_have_accepted_work",
            Self::RestoreOriginalsAfterCandidateQuiesced => {
                "restore_originals_after_candidate_quiesced"
            }
            Self::RestartOriginals => "restart_originals",
            Self::FinishRollback => "finish_rollback",
            Self::DeleteCompleted => "delete_completed",
            Self::QuarantineInvalidJournal => "invalid_journal",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdateRecoveryAgent {
    pub pubkey: String,
    pub name: String,
}

/// Sanitized interrupted-update state for the recovery UI.
///
/// Record snapshots, environment values, credentials, runtime nonces, and
/// handoff identities deliberately remain private to the backend journal.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateUpdateRecoveryStatus {
    pub transaction_id: Option<String>,
    pub template_id: Option<String>,
    pub stage: Option<UpdateTransactionStage>,
    pub recovery: String,
    pub agents: Vec<AgentTemplateUpdateRecoveryAgent>,
    pub requires_attention: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuarantineReason {
    InvalidName,
    LinkOrNonFile,
    InvalidPermissions,
    Oversized,
    InvalidDocument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuarantinedJournal {
    pub(crate) source_name: String,
    pub(crate) reason: QuarantineReason,
}

#[derive(Clone, PartialEq)]
pub(crate) enum RecoveryJournalEntry {
    Transaction(Box<UpdateTransaction>, RecoveryDecision),
    Quarantine(QuarantinedJournal, RecoveryDecision),
}

#[derive(Debug, Clone)]
pub(crate) struct UpdateTransactionJournal {
    active_dir: PathBuf,
    quarantine_dir: PathBuf,
}

impl UpdateTransactionJournal {
    pub(crate) fn for_app(app: &AppHandle) -> Result<Self, String> {
        Self::open_at(&managed_agents_base_dir(app)?)
    }

    pub(crate) fn open_at(agents_dir: &Path) -> Result<Self, String> {
        ensure_private_anchor(agents_dir)?;
        let root = agents_dir.join(JOURNAL_DIR);
        ensure_private_directory(&root)?;
        let active_dir = root.join(ACTIVE_DIR);
        let quarantine_dir = root.join(QUARANTINE_DIR);
        ensure_private_directory(&active_dir)?;
        ensure_private_directory(&quarantine_dir)?;
        Ok(Self {
            active_dir,
            quarantine_dir,
        })
    }

    pub(crate) fn create(&self, transaction: &UpdateTransaction) -> Result<(), String> {
        self.verify_dirs()?;
        transaction.validate()?;
        let path = self.path_for_id(&transaction.transaction_id)?;
        let bytes = serialize_transaction(transaction)?;
        atomic_create_owner_file(&path, &bytes)
    }

    pub(crate) fn load(&self, transaction_id: &str) -> Result<UpdateTransaction, String> {
        self.verify_dirs()?;
        let path = self.path_for_id(transaction_id)?;
        self.load_path(&path)
    }

    #[cfg(test)]
    pub(crate) fn list(&self) -> Result<Vec<UpdateTransaction>, String> {
        self.verify_dirs()?;
        let mut transactions = Vec::new();
        for entry in read_entries(&self.active_dir)? {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "The update journal contains a non-Unicode filename.".to_string())?;
            let id = transaction_id_from_filename(&name)?;
            transactions.push(self.load(&id)?);
        }
        transactions.sort_by(|left, right| left.transaction_id.cmp(&right.transaction_id));
        Ok(transactions)
    }

    pub(crate) fn update(
        &self,
        transaction_id: &str,
        expected_revision: u64,
        stage: UpdateTransactionStage,
        candidate_generations: Vec<JournalRuntimeGeneration>,
    ) -> Result<UpdateTransaction, String> {
        let previous = self.load(transaction_id)?;
        if previous.revision != expected_revision {
            return Err(
                "The update transaction changed before this journal write completed.".to_string(),
            );
        }
        let mut next = previous.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| "The update transaction revision is exhausted.".to_string())?;
        next.updated_at_unix_secs = unix_now_secs().max(previous.updated_at_unix_secs);
        next.stage = stage;
        next.candidate_generations = candidate_generations;
        next.validate_update_from(&previous)?;
        self.write(&self.path_for_id(transaction_id)?, &next)?;
        Ok(next)
    }

    pub(crate) fn delete(
        &self,
        transaction_id: &str,
        expected_revision: u64,
    ) -> Result<(), String> {
        let transaction = self.load(transaction_id)?;
        if transaction.revision != expected_revision {
            return Err("The update transaction changed before it could be removed.".to_string());
        }
        if !matches!(
            transaction.stage,
            UpdateTransactionStage::Prepared
                | UpdateTransactionStage::UpdateCompleted
                | UpdateTransactionStage::RollbackCompleted
        ) {
            return Err(
                "An unresolved update transaction cannot be removed before recovery.".to_string(),
            );
        }
        let path = self.path_for_id(transaction_id)?;
        remove_regular_owner_file(&path)?;
        sync_directory(&self.active_dir)
    }

    pub(crate) fn recovery_entries(&self) -> Result<Vec<RecoveryJournalEntry>, String> {
        self.verify_dirs()?;
        let mut entries = Vec::new();
        for entry in read_entries(&self.active_dir)? {
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => {
                    entries.push(RecoveryJournalEntry::Quarantine(
                        QuarantinedJournal {
                            source_name: "<non-unicode>".to_string(),
                            reason: QuarantineReason::InvalidName,
                        },
                        RecoveryDecision::QuarantineInvalidJournal,
                    ));
                    continue;
                }
            };
            let reason = match transaction_id_from_filename(&name) {
                Ok(id) => match self.load(&id) {
                    Ok(transaction) => {
                        let decision = transaction.recovery_decision();
                        entries.push(RecoveryJournalEntry::Transaction(
                            Box::new(transaction),
                            decision,
                        ));
                        continue;
                    }
                    Err(_) => classify_invalid_path(&entry.path()),
                },
                Err(_) => QuarantineReason::InvalidName,
            };
            entries.push(RecoveryJournalEntry::Quarantine(
                QuarantinedJournal {
                    source_name: name,
                    reason,
                },
                RecoveryDecision::QuarantineInvalidJournal,
            ));
        }
        entries.sort_by(|left, right| recovery_entry_name(left).cmp(recovery_entry_name(right)));
        Ok(entries)
    }

    /// Move one invalid active entry aside without following it.
    #[cfg(test)]
    pub(crate) fn quarantine(&self, source_name: &str) -> Result<PathBuf, String> {
        self.verify_dirs()?;
        validate_leaf_name(source_name)?;
        let source = self.active_dir.join(source_name);
        let _metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("Could not inspect the update journal entry: {error}"))?;
        let destination = self
            .quarantine_dir
            .join(format!("{}.bad", uuid::Uuid::new_v4()));
        fs::rename(&source, &destination)
            .map_err(|error| format!("Could not quarantine the update journal entry: {error}"))?;
        sync_directory(&self.active_dir)?;
        sync_directory(&self.quarantine_dir)?;
        Ok(destination)
    }

    fn path_for_id(&self, transaction_id: &str) -> Result<PathBuf, String> {
        validate_canonical_uuid(transaction_id, "transaction id")?;
        Ok(self.active_dir.join(format!("{transaction_id}.json")))
    }

    fn verify_dirs(&self) -> Result<(), String> {
        ensure_private_directory(&self.active_dir)?;
        ensure_private_directory(&self.quarantine_dir)
    }

    fn load_path(&self, path: &Path) -> Result<UpdateTransaction, String> {
        let bytes = read_owner_file(path, JOURNAL_MAX_BYTES)?;
        let transaction: UpdateTransaction = serde_json::from_slice(&bytes)
            .map_err(|_| "The update transaction document is invalid.".to_string())?;
        transaction.validate()?;
        if serialize_transaction(&transaction)? != bytes {
            return Err("The update transaction document is not canonical.".to_string());
        }
        let expected_name = format!("{}.json", transaction.transaction_id);
        if path.file_name().and_then(|name| name.to_str()) != Some(&expected_name) {
            return Err("The update transaction filename does not match its id.".to_string());
        }
        Ok(transaction)
    }

    fn write(&self, path: &Path, transaction: &UpdateTransaction) -> Result<(), String> {
        let bytes = serialize_transaction(transaction)?;
        atomic_write_owner_file(path, &bytes)
    }
}

fn recovery_status(entry: &RecoveryJournalEntry) -> AgentTemplateUpdateRecoveryStatus {
    match entry {
        RecoveryJournalEntry::Transaction(transaction, decision) => {
            let agents = transaction
                .selected
                .iter()
                .map(|(pubkey, records)| AgentTemplateUpdateRecoveryAgent {
                    pubkey: pubkey.clone(),
                    name: records.original.name.clone(),
                })
                .collect();
            AgentTemplateUpdateRecoveryStatus {
                transaction_id: Some(transaction.transaction_id.clone()),
                template_id: Some(transaction.target.persona_id.clone()),
                stage: Some(transaction.stage),
                recovery: decision.label().to_string(),
                agents,
                requires_attention: !matches!(
                    decision,
                    RecoveryDecision::DiscardUnstarted | RecoveryDecision::DeleteCompleted
                ),
                detail: match decision {
                    RecoveryDecision::QuarantineCandidateMayHaveAcceptedWork => {
                        "An updated agent may have accepted work. Buzz blocked the affected agents instead of guessing which version owns that work."
                    }
                    RecoveryDecision::RestoreOriginalsAfterCandidateQuiesced
                    | RecoveryDecision::RestoreOriginals
                    | RecoveryDecision::RestartOriginals
                    | RecoveryDecision::FinishRollback => {
                        "Buzz found an interrupted update and blocked the affected agents until recovery finishes."
                    }
                    RecoveryDecision::DiscardUnstarted | RecoveryDecision::DeleteCompleted => {
                        "This journal can be cleared without changing a running agent."
                    }
                    RecoveryDecision::QuarantineInvalidJournal => {
                        "The update journal is invalid and its agent scope cannot be trusted."
                    }
                }
                .to_string(),
            }
        }
        RecoveryJournalEntry::Quarantine(quarantined, decision) => {
            AgentTemplateUpdateRecoveryStatus {
                transaction_id: None,
                template_id: None,
                stage: None,
                recovery: decision.label().to_string(),
                agents: Vec::new(),
                requires_attention: true,
                detail: format!(
                    "Buzz could not verify update journal entry {} ({:?}). Agent changes and starts are blocked until it is reviewed.",
                    quarantined.source_name, quarantined.reason
                ),
            }
        }
    }
}

pub(crate) fn list_agent_template_update_recoveries(
    app: &AppHandle,
) -> Result<Vec<AgentTemplateUpdateRecoveryStatus>, String> {
    let journal = UpdateTransactionJournal::for_app(app)?;
    journal
        .recovery_entries()
        .map(|entries| entries.iter().map(recovery_status).collect())
}

/// Reconstruct fail-closed ownership before any startup record mutation or
/// process sweep. Safe no-op and terminal journals are removed; every
/// unresolved identity remains durable and fenced for the recovery UI.
pub(crate) fn initialize_agent_template_update_recovery(
    app: &AppHandle,
) -> Result<Vec<AgentTemplateUpdateRecoveryStatus>, String> {
    const GLOBAL_RECOVERY_OWNER: &str = "recovery-global";

    let state = app.state::<crate::app_state::AppState>();
    let journal = match UpdateTransactionJournal::for_app(app) {
        Ok(journal) => journal,
        Err(error) => {
            state
                .managed_agent_update_leases
                .block_all_for_recovery(GLOBAL_RECOVERY_OWNER)?;
            return Err(error);
        }
    };
    let entries = match journal.recovery_entries() {
        Ok(entries) => entries,
        Err(error) => {
            state
                .managed_agent_update_leases
                .block_all_for_recovery(GLOBAL_RECOVERY_OWNER)?;
            return Err(error);
        }
    };
    let mut unresolved = Vec::new();
    for entry in entries {
        match &entry {
            RecoveryJournalEntry::Transaction(
                transaction,
                RecoveryDecision::DiscardUnstarted | RecoveryDecision::DeleteCompleted,
            ) => {
                journal.delete(&transaction.transaction_id, transaction.revision)?;
            }
            RecoveryJournalEntry::Transaction(transaction, _) => {
                state
                    .managed_agent_update_leases
                    .block_for_recovery(
                        &transaction.transaction_id,
                        transaction.selected.keys().map(String::as_str),
                    )
                    .inspect_err(|_| {
                        let _ = state
                            .managed_agent_update_leases
                            .block_all_for_recovery(GLOBAL_RECOVERY_OWNER);
                    })?;
                unresolved.push(recovery_status(&entry));
            }
            RecoveryJournalEntry::Quarantine(_, _) => {
                state
                    .managed_agent_update_leases
                    .block_all_for_recovery(GLOBAL_RECOVERY_OWNER)?;
                unresolved.push(recovery_status(&entry));
            }
        }
    }
    Ok(unresolved)
}

fn serialize_transaction(transaction: &UpdateTransaction) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(transaction)
        .map_err(|_| "The update transaction could not be serialized.".to_string())?;
    if bytes.is_empty() || bytes.len() as u64 > JOURNAL_MAX_BYTES {
        return Err("The update transaction document is too large.".to_string());
    }
    Ok(bytes)
}

fn candidate_generations_extend(
    previous: &[JournalRuntimeGeneration],
    next: &[JournalRuntimeGeneration],
) -> bool {
    next.len() >= previous.len() && next.starts_with(previous)
}

fn stage_requires_candidate_plan(stage: UpdateTransactionStage) -> bool {
    matches!(
        stage,
        UpdateTransactionStage::BeforeCandidateLaunch
            | UpdateTransactionStage::AfterCandidateReady
            | UpdateTransactionStage::BeforeCandidateHandoff
            | UpdateTransactionStage::AfterCandidateHandoff
            | UpdateTransactionStage::UpdateCompleted
    )
}

fn generation_keys(generations: &[JournalRuntimeGeneration]) -> HashSet<ManagedAgentRuntimeKey> {
    generations
        .iter()
        .map(|generation| generation.key.clone())
        .collect()
}

fn validate_generations(
    generations: &[JournalRuntimeGeneration],
    selected: &BTreeMap<String, SelectedAgentRecords>,
    label: &str,
) -> Result<(), String> {
    if generations.len() > JOURNAL_MAX_GENERATIONS {
        return Err(format!("The update transaction has too many {label}s."));
    }
    let mut keys = HashSet::with_capacity(generations.len());
    for generation in generations {
        let canonical =
            ManagedAgentRuntimeKey::new(&generation.key.pubkey, &generation.key.relay_url)?;
        if canonical != generation.key
            || !selected.contains_key(&generation.key.pubkey)
            || !keys.insert(generation.key.clone())
            || !is_lower_hex(&generation.start_nonce, 32)
            || generation.handoff.start_nonce != generation.start_nonce
        {
            return Err(format!(
                "The update transaction contains an invalid {label} generation."
            ));
        }
        validate_canonical_uuid(&generation.handoff.handoff_id, "handoff id")?;
    }
    Ok(())
}

fn validate_pubkey(value: &str) -> Result<(), String> {
    if is_lower_hex(value, 64) {
        Ok(())
    } else {
        Err("The update transaction contains an invalid agent key.".to_string())
    }
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_canonical_uuid(value: &str, label: &str) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| format!("The {label} is invalid."))?;
    if parsed.hyphenated().to_string() == value {
        Ok(())
    } else {
        Err(format!("The {label} is invalid."))
    }
}

fn validate_safe_id(value: &str, label: &str) -> Result<(), String> {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Ok(())
    } else {
        Err(format!("The update transaction {label} is invalid."))
    }
}

fn transaction_id_from_filename(name: &str) -> Result<String, String> {
    let id = name
        .strip_suffix(".json")
        .ok_or_else(|| "The update journal contains an invalid filename.".to_string())?;
    validate_canonical_uuid(id, "transaction filename")?;
    Ok(id.to_string())
}

#[cfg(test)]
fn validate_leaf_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || Path::new(name).components().count() != 1
    {
        Err("The update journal filename is invalid.".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "update_transaction_tests.rs"]
mod tests;
