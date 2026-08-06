use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
};

const STALE_OPERATION_ERROR: &str =
    "This Project connection operation was cancelled because the workspace changed.";

#[derive(Default)]
pub(crate) struct ScopeOperationCoordinator {
    entries: Mutex<HashMap<u64, Arc<ScopeOperationEntry>>>,
    retired_through: AtomicU64,
}

#[derive(Default)]
struct ScopeOperationEntry {
    cancelled: AtomicBool,
    active: Mutex<usize>,
    drained: Condvar,
}

pub(crate) struct ScopeOperationLease {
    entry: Arc<ScopeOperationEntry>,
}

impl ScopeOperationCoordinator {
    /// Acquire an operation lease while the caller holds `workspace_transition`.
    ///
    /// That outer serialization is what makes capture linearizable with
    /// `cancel_and_drain`: after revocation begins, no command can acquire a
    /// fresh lease for the generation being retired.
    pub(crate) fn acquire(&self, generation: u64) -> Result<ScopeOperationLease, String> {
        if generation <= self.retired_through.load(Ordering::Acquire) {
            return Err(STALE_OPERATION_ERROR.to_string());
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Buzz could not coordinate Project connection operations.".to_string())?;
        if generation <= self.retired_through.load(Ordering::Acquire) {
            return Err(STALE_OPERATION_ERROR.to_string());
        }
        let entry = entries
            .entry(generation)
            .or_insert_with(|| Arc::new(ScopeOperationEntry::default()))
            .clone();
        if entry.cancelled.load(Ordering::Acquire) {
            return Err(STALE_OPERATION_ERROR.to_string());
        }
        let mut active = entry
            .active
            .lock()
            .map_err(|_| "Buzz could not coordinate Project connection operations.".to_string())?;
        if entry.cancelled.load(Ordering::Acquire) {
            return Err(STALE_OPERATION_ERROR.to_string());
        }
        *active += 1;
        drop(active);
        drop(entries);
        Ok(ScopeOperationLease { entry })
    }

    /// Revoke a generation, cancel its probes, and wait for every external
    /// side effect to finish before the workspace's in-memory scope is swapped.
    ///
    /// Probe waits poll the lease and terminate their process group promptly.
    /// OS keyring calls cannot be interrupted, so a keyring prompt already in
    /// progress must finish before this drain can complete.
    pub(crate) fn cancel_and_drain(&self, generation: u64) {
        self.retired_through.fetch_max(generation, Ordering::AcqRel);
        let entry = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&generation)
            .cloned();
        let Some(entry) = entry else {
            return;
        };
        entry.cancelled.store(true, Ordering::Release);
        let mut active = entry
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *active != 0 {
            active = entry
                .drained
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(active);
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries
            .get(&generation)
            .is_some_and(|candidate| Arc::ptr_eq(candidate, &entry))
        {
            entries.remove(&generation);
        }
    }
}

impl ScopeOperationLease {
    pub(crate) fn check_active(&self) -> Result<(), String> {
        if self.entry.cancelled.load(Ordering::Acquire) {
            Err(STALE_OPERATION_ERROR.to_string())
        } else {
            Ok(())
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.entry.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for ScopeOperationLease {
    fn drop(&mut self) {
        let mut active = self
            .entry
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        debug_assert!(*active > 0);
        *active = active.saturating_sub(1);
        if *active == 0 {
            self.entry.drained.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoke_drains_and_permanently_rejects_the_generation() {
        let coordinator = Arc::new(ScopeOperationCoordinator::default());
        let lease = coordinator.acquire(7).unwrap();
        let revoker = {
            let coordinator = Arc::clone(&coordinator);
            std::thread::spawn(move || coordinator.cancel_and_drain(7))
        };
        while !lease.is_cancelled() {
            std::thread::yield_now();
        }
        assert!(lease.check_active().is_err());
        drop(lease);
        revoker.join().unwrap();
        assert!(coordinator.acquire(7).is_err());
        assert!(coordinator.acquire(8).is_ok());
    }

    #[test]
    fn a_to_b_to_a_uses_distinct_generation_leases() {
        let coordinator = ScopeOperationCoordinator::default();
        let a1 = coordinator.acquire(11).unwrap();
        drop(a1);
        coordinator.cancel_and_drain(11);
        let b = coordinator.acquire(12).unwrap();
        drop(b);
        coordinator.cancel_and_drain(12);
        let a2 = coordinator.acquire(13).unwrap();
        assert!(a2.check_active().is_ok());
        assert!(coordinator.acquire(11).is_err());
    }
}
