use std::{path::PathBuf, process::Child};

use super::AcpAvailabilityStatus;

#[derive(Debug)]
pub struct ManagedAgentProcess {
    pub child: Child,
    pub log_path: PathBuf,
    /// Digest of the effective spawn config at launch (see
    /// `spawn_hash::spawn_config_hash`). Runtime-only — never persisted. The
    /// summary builder recomputes the hash from current disk state and flags
    /// `needs_restart` on mismatch. Agents adopted via a persisted
    /// `runtime_pid` have no `ManagedAgentProcess` entry, so their spawn
    /// config is unknown and the badge stays off.
    pub spawn_config_hash: u64,
    /// Digest of non-secret Project connection generation counters used by
    /// this process. It changes when transport or credentials are updated,
    /// without hashing credential values.
    pub connection_generation_hash: u64,
    /// Owner-only generated MCP config handed to buzz-acp. The harness deletes
    /// it after reading; Desktop also removes it during stop as a cleanup
    /// fallback.
    pub connection_config_path: Option<PathBuf>,
    /// Whether this process was spawned in setup-listener mode (i.e.
    /// `BUZZ_ACP_SETUP_PAYLOAD` was set at launch because the agent was
    /// `NotReady`). Runtime-only — never persisted. Used by
    /// `install_acp_runtime` to target only stuck agents for auto-restart,
    /// excluding healthy in-pool agents.
    pub setup_mode: bool,
    /// Adapter availability status stamped at spawn time for runtimes with a
    /// version gate (currently codex only; `None` for all others). Runtime-only
    /// — never persisted. The summary builder compares this against the current
    /// cached availability and sets `needs_restart` on drift, catching out-of-
    /// band adapter changes that Phase-1 auto-restart doesn't cover.
    pub adapter_availability: Option<AcpAvailabilityStatus>,
    /// Unpredictable identity shared only with this harness generation.
    pub start_nonce: String,
    /// Win32 Job Object owning the harness + its entire process tree. Closing
    /// the handle (via `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) kills the whole
    /// tree — the Windows mirror of the Unix process-group teardown. `None`
    /// if job creation/assignment failed (we fall back to `Child::kill()`).
    #[cfg(windows)]
    pub job: Option<crate::managed_agents::JobHandle>,
}
