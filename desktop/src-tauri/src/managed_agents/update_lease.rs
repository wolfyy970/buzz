use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, MutexGuard},
};

/// Process-local, pubkey-scoped ownership for managed-agent configuration work.
///
/// A lease is independent of runtime rows, so stopping the final runtime pair
/// cannot accidentally make an in-flight update claim disappear. All pubkeys
/// in a request are checked and inserted while holding one mutex, which makes
/// overlapping multi-agent operations fail atomically.
#[derive(Debug, Default)]
pub struct ManagedAgentUpdateLeaseRegistry {
    state: Mutex<ManagedAgentLeaseState>,
}

#[derive(Debug, Default)]
struct ManagedAgentLeaseState {
    owners: HashMap<String, String>,
    global_owner: Option<String>,
    recovery_owners: HashMap<String, String>,
    recovery_global_owner: Option<String>,
}

/// RAII ownership of one atomic set of managed-agent pubkeys.
///
/// Dropping the lease releases only entries still owned by its operation ID.
/// The owner check makes cleanup safe even if a poisoned-lock recovery or
/// future explicit transfer changes an entry before the original guard drops.
#[derive(Debug)]
#[must_use = "dropping the lease immediately releases managed-agent ownership"]
pub struct ManagedAgentUpdateLease {
    registry: Arc<ManagedAgentUpdateLeaseRegistry>,
    operation_id: String,
    pubkeys: Vec<String>,
    global: bool,
}

/// Keeps lease ownership stable from the storage authorization check through
/// the corresponding atomic file replacement.
#[must_use = "the managed-agent store write must remain fenced until it finishes"]
pub(crate) struct ManagedAgentStoreWriteGuard<'a> {
    _state: MutexGuard<'a, ManagedAgentLeaseState>,
}

impl ManagedAgentUpdateLeaseRegistry {
    fn lock_state(&self) -> std::sync::MutexGuard<'_, ManagedAgentLeaseState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Atomically reserve every requested pubkey for `operation_id`.
    pub fn try_acquire<I, S>(
        self: &Arc<Self>,
        operation_id: impl Into<String>,
        pubkeys: I,
    ) -> Result<ManagedAgentUpdateLease, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let operation_id = operation_id.into();
        validate_operation_id(&operation_id)?;
        let pubkeys = canonical_pubkey_set(pubkeys)?;
        if pubkeys.is_empty() {
            return Err("managed-agent update lease requires at least one agent".to_string());
        }

        let mut state = self.lock_state();
        if state.recovery_global_owner.is_some() {
            return Err(
                "Agent updates are blocked until an interrupted update is recovered.".to_string(),
            );
        }
        if let Some(owner) = state.global_owner.as_deref() {
            eprintln!("buzz-desktop: managed-agent lease conflict with global owner {owner}");
            return Err(
                "Agent settings are being updated. Try again when the update finishes.".to_string(),
            );
        }
        if let Some((pubkey, owner)) = pubkeys
            .iter()
            .find_map(|pubkey| state.owners.get(pubkey).map(|owner| (pubkey, owner)))
        {
            eprintln!("buzz-desktop: managed-agent lease conflict for {pubkey} (owner {owner})");
            return Err(
                "This agent is being updated. Try again when the update finishes.".to_string(),
            );
        }
        if pubkeys
            .iter()
            .any(|pubkey| state.recovery_owners.contains_key(pubkey))
        {
            return Err(
                "This agent is blocked until its interrupted update is recovered.".to_string(),
            );
        }
        for pubkey in &pubkeys {
            state.owners.insert(pubkey.clone(), operation_id.clone());
        }

        Ok(ManagedAgentUpdateLease {
            registry: Arc::clone(self),
            operation_id,
            pubkeys,
            global: false,
        })
    }

    /// Reserve the affected pubkeys for an ordinary configuration mutation.
    ///
    /// Empty target sets are a valid no-op (for example, editing a persona
    /// that currently has no instances), hence the optional guard.
    pub fn try_acquire_mutation<I, S>(
        self: &Arc<Self>,
        label: &str,
        pubkeys: I,
    ) -> Result<Option<ManagedAgentUpdateLease>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let label = label.trim();
        if label.is_empty() {
            return Err("managed-agent mutation label cannot be empty".to_string());
        }
        let pubkeys = canonical_pubkey_set(pubkeys)?;
        if pubkeys.is_empty() {
            return Ok(None);
        }
        let operation_id = format!("mutation:{label}:{}", uuid::Uuid::new_v4());
        self.try_acquire(operation_id, pubkeys).map(Some)
    }

    /// Reserve the complete managed-agent mutation namespace.
    ///
    /// Global configuration and project-connection catalog mutations can
    /// affect agents not known when their command begins. This single mutex
    /// boundary prevents both existing pubkey leases and new acquisitions.
    pub fn try_acquire_global_mutation(
        self: &Arc<Self>,
        label: &str,
    ) -> Result<ManagedAgentUpdateLease, String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("managed-agent mutation label cannot be empty".to_string());
        }
        let operation_id = format!("mutation-global:{label}:{}", uuid::Uuid::new_v4());
        validate_operation_id(&operation_id)?;
        let mut state = self.lock_state();
        if state.global_owner.is_some()
            || !state.owners.is_empty()
            || state.recovery_global_owner.is_some()
            || !state.recovery_owners.is_empty()
        {
            return Err(
                "Agent settings are being updated. Try again when the update finishes.".to_string(),
            );
        }
        state.global_owner = Some(operation_id.clone());
        Ok(ManagedAgentUpdateLease {
            registry: Arc::clone(self),
            operation_id,
            pubkeys: Vec::new(),
            global: true,
        })
    }

    /// Reserve every currently available pubkey with one operation ID,
    /// returning busy pubkeys for a best-effort caller to skip.
    ///
    /// Launch restore uses this shape so one agent already in a safe update
    /// does not prevent unrelated agents from restoring, while the successful
    /// subset can still be persisted by one operation-aware store write.
    pub fn try_acquire_available_mutation<I, S>(
        self: &Arc<Self>,
        label: &str,
        pubkeys: I,
    ) -> Result<(Option<ManagedAgentUpdateLease>, Vec<String>), String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let label = label.trim();
        if label.is_empty() {
            return Err("managed-agent mutation label cannot be empty".to_string());
        }
        let pubkeys = canonical_pubkey_set(pubkeys)?;
        let operation_id = format!("mutation:{label}:{}", uuid::Uuid::new_v4());
        validate_operation_id(&operation_id)?;
        let mut state = self.lock_state();
        if state.global_owner.is_some() || state.recovery_global_owner.is_some() {
            return Ok((None, pubkeys));
        }

        let (busy, available): (Vec<_>, Vec<_>) = pubkeys.into_iter().partition(|pubkey| {
            state.owners.contains_key(pubkey) || state.recovery_owners.contains_key(pubkey)
        });
        if available.is_empty() {
            return Ok((None, busy));
        }
        for pubkey in &available {
            state.owners.insert(pubkey.clone(), operation_id.clone());
        }
        Ok((
            Some(ManagedAgentUpdateLease {
                registry: Arc::clone(self),
                operation_id,
                pubkeys: available,
                global: false,
            }),
            busy,
        ))
    }

    /// Require that `operation_id` owns every requested pubkey.
    ///
    /// Update-internal runtime starts use this instead of acquiring a second,
    /// self-conflicting lease.
    pub fn ensure_owned<I, S>(&self, operation_id: &str, pubkeys: I) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        validate_operation_id(operation_id)?;
        let pubkeys = canonical_pubkey_set(pubkeys)?;
        let state = self.lock_state();
        for pubkey in pubkeys {
            if state.recovery_global_owner.as_deref() == Some(operation_id) {
                continue;
            }
            if let Some(owner) = state.recovery_owners.get(&pubkey) {
                if owner == operation_id {
                    continue;
                }
                return Err(
                    "The managed-agent update is blocked by interrupted recovery.".to_string(),
                );
            }
            if state.global_owner.as_deref() == Some(operation_id) {
                continue;
            }
            match state.owners.get(&pubkey) {
                Some(owner) if owner == operation_id => {}
                Some(owner) => {
                    eprintln!(
                        "buzz-desktop: managed-agent authorization mismatch for {pubkey} \
                         (owner {owner}, caller {operation_id})"
                    );
                    return Err("The managed-agent update no longer owns this agent.".to_string());
                }
                None => {
                    eprintln!(
                        "buzz-desktop: managed-agent authorization missing for {pubkey} \
                         (caller {operation_id})"
                    );
                    return Err("The managed-agent update no longer owns this agent.".to_string());
                }
            }
        }
        Ok(())
    }

    /// Authorize a store write and keep ownership unchanged until it commits.
    ///
    /// A check that releases this mutex before the file replacement would
    /// leave a race in which a safe update could acquire a changed pubkey
    /// between authorization and persistence.
    pub(crate) fn acquire_store_write_guard<I, S>(
        &self,
        operation_id: Option<&str>,
        changed_pubkeys: I,
    ) -> Result<ManagedAgentStoreWriteGuard<'_>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if let Some(operation_id) = operation_id {
            validate_operation_id(operation_id)?;
        }
        let changed_pubkeys = canonical_pubkey_set(changed_pubkeys)?;
        let state = self.lock_state();
        check_store_write_allowed(&state, operation_id, changed_pubkeys)?;
        Ok(ManagedAgentStoreWriteGuard { _state: state })
    }

    /// Keep selected agents fenced after an interrupted transaction outlives
    /// its stack-local lease. Re-registering the same journal is idempotent.
    pub(crate) fn block_for_recovery<I, S>(
        &self,
        operation_id: &str,
        pubkeys: I,
    ) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        validate_operation_id(operation_id)?;
        let pubkeys = canonical_pubkey_set(pubkeys)?;
        if pubkeys.is_empty() {
            return Err("managed-agent recovery requires at least one agent".to_string());
        }
        let mut state = self.lock_state();
        if state
            .recovery_global_owner
            .as_deref()
            .is_some_and(|owner| owner != operation_id)
        {
            return Err("A global interrupted-update recovery block is active.".to_string());
        }
        for pubkey in &pubkeys {
            if state
                .recovery_owners
                .get(pubkey)
                .is_some_and(|owner| owner != operation_id)
            {
                return Err("Overlapping interrupted updates require global recovery.".to_string());
            }
        }
        for pubkey in pubkeys {
            state
                .recovery_owners
                .insert(pubkey, operation_id.to_string());
        }
        Ok(())
    }

    /// Fail closed across the managed-agent namespace when journal scope is
    /// invalid or unreadable and affected identities cannot be trusted.
    pub(crate) fn block_all_for_recovery(&self, operation_id: &str) -> Result<(), String> {
        validate_operation_id(operation_id)?;
        let mut state = self.lock_state();
        match state.recovery_global_owner.as_deref() {
            Some(owner) if owner != operation_id => {
                Err("A different global interrupted-update recovery block is active.".to_string())
            }
            _ => {
                state.recovery_global_owner = Some(operation_id.to_string());
                Ok(())
            }
        }
    }

    /// Release only the transaction-specific recovery ownership that was
    /// durably resolved and removed from the journal.
    pub(crate) fn clear_recovery<I, S>(&self, operation_id: &str, pubkeys: I) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        validate_operation_id(operation_id)?;
        let pubkeys = canonical_pubkey_set(pubkeys)?;
        let mut state = self.lock_state();
        for pubkey in &pubkeys {
            match state.recovery_owners.get(pubkey) {
                Some(owner) if owner == operation_id => {}
                Some(_) => {
                    return Err(
                        "A different interrupted update owns this agent recovery.".to_string()
                    );
                }
                None => {
                    return Err(
                        "The interrupted update no longer owns this agent recovery.".to_string()
                    );
                }
            }
        }
        for pubkey in pubkeys {
            state.recovery_owners.remove(&pubkey);
        }
        Ok(())
    }

    #[cfg(test)]
    fn owner_for(&self, pubkey: &str) -> Option<String> {
        self.lock_state()
            .owners
            .get(&pubkey.to_ascii_lowercase())
            .cloned()
    }
}

fn check_store_write_allowed(
    state: &ManagedAgentLeaseState,
    operation_id: Option<&str>,
    changed_pubkeys: Vec<String>,
) -> Result<(), String> {
    for pubkey in changed_pubkeys {
        if let Some(owner) = state.recovery_global_owner.as_deref() {
            if operation_id != Some(owner) {
                return Err(
                    "Agent updates are blocked until an interrupted update is recovered."
                        .to_string(),
                );
            }
        }
        if let Some(owner) = state.recovery_owners.get(&pubkey) {
            if operation_id != Some(owner.as_str()) {
                return Err(
                    "This agent is blocked until its interrupted update is recovered.".to_string(),
                );
            }
        }
        if let Some(global_owner) = state.global_owner.as_deref() {
            if operation_id == Some(global_owner) {
                continue;
            }
            return Err(
                "Agent settings are being updated. Try again when the update finishes.".to_string(),
            );
        }
        match (operation_id, state.owners.get(&pubkey)) {
            (None, None) => {}
            (None, Some(owner)) => {
                eprintln!(
                    "buzz-desktop: managed-agent store write fenced for {pubkey} \
                         (owner {owner})"
                );
                return Err(
                    "This agent is being updated. Try again when the update finishes.".to_string(),
                );
            }
            (Some(operation_id), Some(owner)) if owner == operation_id => {}
            (Some(operation_id), Some(owner)) => {
                eprintln!(
                    "buzz-desktop: managed-agent store authorization mismatch for {pubkey} \
                         (owner {owner}, caller {operation_id})"
                );
                return Err("The managed-agent update no longer owns this agent.".to_string());
            }
            (Some(_), None) => {}
        }
    }
    Ok(())
}

impl ManagedAgentUpdateLease {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn pubkeys(&self) -> &[String] {
        &self.pubkeys
    }
}

impl Drop for ManagedAgentUpdateLease {
    fn drop(&mut self) {
        let mut state = self.registry.lock_state();
        if self.global
            && state
                .global_owner
                .as_deref()
                .is_some_and(|owner| owner == self.operation_id)
        {
            state.global_owner = None;
        }
        for pubkey in &self.pubkeys {
            if state
                .owners
                .get(pubkey)
                .is_some_and(|owner| owner == &self.operation_id)
            {
                state.owners.remove(pubkey);
            }
        }
    }
}

fn canonical_pubkey_set<I, S>(pubkeys: I) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut canonical = BTreeSet::new();
    for pubkey in pubkeys {
        let pubkey = pubkey.as_ref().trim().to_ascii_lowercase();
        nostr::PublicKey::from_hex(&pubkey)
            .map_err(|error| format!("invalid managed-agent pubkey: {error}"))?;
        canonical.insert(pubkey);
    }
    Ok(canonical.into_iter().collect())
}

fn validate_operation_id(operation_id: &str) -> Result<(), String> {
    if operation_id.is_empty()
        || operation_id.len() > 128
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err("managed-agent operation id is invalid".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_agents_still_reject_overlapping_update_leases() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let pubkey = "aa".repeat(32);
        let first = registry
            .try_acquire("update-one", [pubkey.as_str()])
            .expect("first lease");

        let error = registry
            .try_acquire("update-two", [pubkey.as_str()])
            .expect_err("overlap must fail without a runtime row");
        assert!(error.contains("being updated"));
        assert_eq!(registry.owner_for(&pubkey).as_deref(), Some("update-one"));

        drop(first);
        let second = registry
            .try_acquire("update-two", [pubkey.as_str()])
            .expect("released lease can be reacquired");
        assert_eq!(second.operation_id(), "update-two");
    }

    #[test]
    fn multi_agent_acquisition_is_all_or_nothing() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let first_pubkey = "aa".repeat(32);
        let second_pubkey = "bb".repeat(32);
        let _first = registry
            .try_acquire("update-one", [first_pubkey.as_str()])
            .expect("first lease");

        assert!(registry
            .try_acquire(
                "update-two",
                [first_pubkey.as_str(), second_pubkey.as_str()]
            )
            .is_err());
        assert_eq!(registry.owner_for(&second_pubkey), None);
    }

    #[test]
    fn authorized_operation_must_match_the_lease_owner() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let pubkey = "aa".repeat(32);
        let _lease = registry
            .try_acquire("update-one", [pubkey.as_str()])
            .expect("lease");

        registry
            .ensure_owned("update-one", [pubkey.as_str()])
            .expect("owner is authorized");
        assert!(registry
            .ensure_owned("update-two", [pubkey.as_str()])
            .is_err());
    }

    #[test]
    fn update_lease_fences_persona_global_and_project_mutations() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let pubkey = "aa".repeat(32);
        let _lease = registry
            .try_acquire("safe-update", [pubkey.as_str()])
            .expect("lease");

        for label in [
            "persona-update",
            "persona-publish",
            "global-config",
            "project-connection",
        ] {
            assert!(
                registry
                    .try_acquire_mutation(label, [pubkey.as_str()])
                    .is_err(),
                "{label} must be fenced"
            );
        }
    }

    #[test]
    fn global_lease_blocks_new_pubkeys_even_when_acquired_with_no_agents() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let global = registry
            .try_acquire_global_mutation("global-config")
            .expect("global lease");
        let new_pubkey = "cc".repeat(32);

        assert!(registry
            .try_acquire("safe-update", [new_pubkey.as_str()])
            .is_err());
        drop(global);
        let _lease = registry
            .try_acquire("safe-update", [new_pubkey.as_str()])
            .expect("global drop releases the namespace");
    }

    #[test]
    fn global_and_pubkey_leases_conflict_in_both_directions() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let pubkey = "aa".repeat(32);
        let lease = registry
            .try_acquire("safe-update", [pubkey.as_str()])
            .expect("pubkey lease");
        assert!(registry
            .try_acquire_global_mutation("global-config")
            .is_err());
        drop(lease);

        let global = registry
            .try_acquire_global_mutation("global-config")
            .expect("global lease");
        assert!(registry
            .try_acquire("safe-update", [pubkey.as_str()])
            .is_err());
        drop(global);
    }

    #[test]
    fn interrupted_recovery_block_survives_the_live_lease_drop() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let pubkey = "aa".repeat(32);
        let lease = registry
            .try_acquire("update-one", [pubkey.as_str()])
            .expect("live update lease");
        registry
            .block_for_recovery("update-one", [pubkey.as_str()])
            .expect("durable recovery ownership");
        drop(lease);

        assert!(registry
            .try_acquire("update-two", [pubkey.as_str()])
            .expect_err("recovery must remain fenced")
            .contains("recovered"));
        registry
            .ensure_owned("update-one", [pubkey.as_str()])
            .expect("the recovery operation remains authorized");
        let write_guard = registry
            .acquire_store_write_guard(Some("update-one"), [pubkey.as_str()])
            .expect("recovery may finish its own store write");
        drop(write_guard);
        registry
            .clear_recovery("update-one", [pubkey.as_str()])
            .expect("terminal recovery clears its fence");
        let _next = registry
            .try_acquire("update-two", [pubkey.as_str()])
            .expect("resolved agent may be updated again");
    }

    #[test]
    fn invalid_journal_global_block_fences_known_and_future_agents() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        registry
            .block_all_for_recovery("recovery-global")
            .expect("global recovery block");

        for pubkey in ["aa".repeat(32), "bb".repeat(32)] {
            assert!(registry
                .try_acquire("ordinary-start", [pubkey.as_str()])
                .is_err());
            assert!(registry
                .acquire_store_write_guard(None, [pubkey.as_str()])
                .is_err());
        }
        assert!(registry
            .try_acquire_global_mutation("global-config")
            .is_err());
    }

    #[test]
    fn overlapping_recovery_journals_fail_before_partial_ownership() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let first = "aa".repeat(32);
        let second = "bb".repeat(32);
        registry
            .block_for_recovery("update-one", [first.as_str()])
            .expect("first recovery");

        assert!(registry
            .block_for_recovery("update-two", [first.as_str(), second.as_str()])
            .is_err());
        assert!(!registry.lock_state().recovery_owners.contains_key(&second));
    }

    #[test]
    fn drop_releases_ownership_after_mutex_poisoning() {
        let registry = Arc::new(ManagedAgentUpdateLeaseRegistry::default());
        let pubkey = "aa".repeat(32);
        let lease = registry
            .try_acquire("safe-update", [pubkey.as_str()])
            .expect("lease");
        let poison_registry = Arc::clone(&registry);
        let _ = std::thread::spawn(move || {
            let _guard = poison_registry
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            panic!("poison registry for drop recovery test");
        })
        .join();

        drop(lease);
        let _next = registry
            .try_acquire("next-update", [pubkey.as_str()])
            .expect("poison recovery must not leak ownership");
    }
}
