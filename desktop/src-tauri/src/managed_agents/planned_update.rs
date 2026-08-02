use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager as _};

use super::{
    append_log_marker, cancel_planned_update_request, planned_update_request_state,
    prepare_managed_agent_handoff, remove_agent_runtime_receipt,
    take_exited_claimed_managed_agent_runtime, verify_handoff_checkpoint,
    verify_managed_agent_runtime_pair_claim, write_planned_update_request, ManagedAgentPairRuntime,
    ManagedAgentRuntimeKey, ManagedAgentRuntimeLifecycle, PlannedUpdateIdentity,
    PlannedUpdateRequestState,
};

const DRAIN_POLL: Duration = Duration::from_millis(200);
const REQUEST_ACCEPT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub(crate) struct ManagedAgentUpdateDrainError {
    pub(crate) message: String,
    /// True only while the original configuration can be restarted without
    /// creating a second writer for this pair.
    pub(crate) rollback_safe: bool,
}

impl std::fmt::Display for ManagedAgentUpdateDrainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl ManagedAgentUpdateDrainError {
    fn before_acceptance(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            rollback_safe: true,
        }
    }

    fn after_acceptance(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            rollback_safe: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DrainedManagedAgentPair;

fn update_can_begin(lifecycle: &ManagedAgentRuntimeLifecycle) -> bool {
    matches!(
        lifecycle,
        ManagedAgentRuntimeLifecycle::Listening
            | ManagedAgentRuntimeLifecycle::Waking
            | ManagedAgentRuntimeLifecycle::Ready
            | ManagedAgentRuntimeLifecycle::Failed
    )
}

fn handoff_exit_is_clean(status: &std::process::ExitStatus) -> bool {
    status.success()
}

fn request_acceptance_timed_out(started: Instant, now: Instant) -> bool {
    now.saturating_duration_since(started) >= REQUEST_ACCEPT_TIMEOUT
}

fn cleanup_exited_runtime(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
    mut runtime: ManagedAgentPairRuntime,
) {
    if let Some(path) = runtime.connection_config_path.take() {
        super::project_connections::remove_runtime_mcp_config(&path);
    }
    remove_agent_runtime_receipt(app, key);
    app.state::<crate::app_state::AppState>()
        .clear_agent_session_cache(key);
    if let Err(error) = append_log_marker(
        &runtime.log_path,
        &format!(
            "=== handed off {} on {} for an update at {} ===",
            key.pubkey,
            key.relay_url,
            crate::util::now_iso()
        ),
    ) {
        eprintln!(
            "buzz-desktop: failed to append update handoff marker for {} on {}: {error}",
            key.pubkey, key.relay_url
        );
    }
}

fn claimed_runtime_snapshot(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
    operation_id: &str,
) -> Result<(String, ManagedAgentRuntimeLifecycle), String> {
    let state = app.state::<crate::app_state::AppState>();
    let runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;
    let claim = verify_managed_agent_runtime_pair_claim(&runtimes, key, operation_id)?;
    let lifecycle = runtimes
        .get(key)
        .ok_or_else(|| "The running agent disappeared before the update began.".to_string())?
        .lifecycle
        .clone();
    Ok((claim.start_nonce, lifecycle))
}

fn take_exited_claimed_runtime(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
    operation_id: &str,
) -> Result<Option<(ManagedAgentPairRuntime, std::process::ExitStatus)>, String> {
    let state = app.state::<crate::app_state::AppState>();
    let mut runtimes = state
        .managed_agent_processes
        .lock()
        .map_err(|error| error.to_string())?;
    take_exited_claimed_managed_agent_runtime(&mut runtimes, key, operation_id)
}

/// Drain one already-claimed ACP runtime through a generation-bound handoff.
///
/// The acceptance timeout applies only while Desktop still owns the request
/// and can cancel it safely. Once ACP consumes the request, this function
/// follows that exact generation to exit and never tells the caller that
/// restarting the old configuration is safe prematurely.
pub(crate) async fn drain_managed_agent_pair_for_update(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
    operation_id: &str,
) -> Result<DrainedManagedAgentPair, ManagedAgentUpdateDrainError> {
    let (start_nonce, lifecycle) = claimed_runtime_snapshot(app, key, operation_id)
        .map_err(ManagedAgentUpdateDrainError::before_acceptance)?;
    if !update_can_begin(&lifecycle) {
        return Err(ManagedAgentUpdateDrainError::before_acceptance(
            match lifecycle {
                ManagedAgentRuntimeLifecycle::Starting => {
                    "This agent is still starting. Wait until it is available, then try again."
                }
                ManagedAgentRuntimeLifecycle::Stopped => {
                    "This agent stopped before the update began. Refresh the affected agents and try again."
                }
                ManagedAgentRuntimeLifecycle::Listening
                | ManagedAgentRuntimeLifecycle::Waking
                | ManagedAgentRuntimeLifecycle::Ready
                | ManagedAgentRuntimeLifecycle::Failed => unreachable!(),
            },
        ));
    }

    let paths = prepare_managed_agent_handoff(app, key)
        .map_err(ManagedAgentUpdateDrainError::before_acceptance)?;
    match std::fs::symlink_metadata(&paths.checkpoint) {
        Ok(_) => {
            return Err(ManagedAgentUpdateDrainError::before_acceptance(
                "This agent is still recovering from its last update. Wait until it is available, then try again.",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ManagedAgentUpdateDrainError::before_acceptance(format!(
                "Could not inspect the agent's prior recovery checkpoint: {error}"
            )));
        }
    }

    let identity = PlannedUpdateIdentity::new(&start_nonce)
        .map_err(ManagedAgentUpdateDrainError::before_acceptance)?;
    let published = write_planned_update_request(&paths.request, &identity)
        .map_err(ManagedAgentUpdateDrainError::before_acceptance)?;
    let started = Instant::now();
    let mut accepted = false;

    loop {
        match take_exited_claimed_runtime(app, key, operation_id) {
            Ok(Some((runtime, status))) => {
                let request_state = planned_update_request_state(&paths.request, &published)
                    .map_err(|error| {
                        if accepted {
                            ManagedAgentUpdateDrainError::after_acceptance(error)
                        } else {
                            ManagedAgentUpdateDrainError::before_acceptance(error)
                        }
                    })?;
                accepted |= request_state == PlannedUpdateRequestState::Accepted;
                if !accepted {
                    cleanup_exited_runtime(app, key, runtime);
                    return Err(ManagedAgentUpdateDrainError::before_acceptance(format!(
                        "The agent exited before accepting the update ({status})."
                    )));
                }
                if !handoff_exit_is_clean(&status) {
                    cleanup_exited_runtime(app, key, runtime);
                    return Err(ManagedAgentUpdateDrainError::after_acceptance(format!(
                        "The agent exited unsuccessfully after accepting the update ({status}); Buzz did not trust its checkpoint."
                    )));
                }
                if let Err(error) = verify_handoff_checkpoint(&paths.checkpoint, key, &identity) {
                    cleanup_exited_runtime(app, key, runtime);
                    return Err(ManagedAgentUpdateDrainError::after_acceptance(error));
                }
                cleanup_exited_runtime(app, key, runtime);
                return Ok(DrainedManagedAgentPair);
            }
            Ok(None) => {}
            Err(error) => {
                if !accepted {
                    match cancel_planned_update_request(&paths.request, &published) {
                        Ok(true) => {
                            return Err(ManagedAgentUpdateDrainError::before_acceptance(error));
                        }
                        Ok(false) => accepted = true,
                        Err(cancel_error) => {
                            return Err(ManagedAgentUpdateDrainError::after_acceptance(format!(
                                "{error} The update request could not be cancelled safely: {cancel_error}"
                            )));
                        }
                    }
                } else {
                    return Err(ManagedAgentUpdateDrainError::after_acceptance(error));
                }
            }
        }

        match planned_update_request_state(&paths.request, &published) {
            Ok(PlannedUpdateRequestState::Accepted) => accepted = true,
            Ok(PlannedUpdateRequestState::Pending) => {}
            Err(error) => {
                return Err(if accepted {
                    ManagedAgentUpdateDrainError::after_acceptance(error)
                } else {
                    ManagedAgentUpdateDrainError::before_acceptance(error)
                });
            }
        }

        if !accepted && request_acceptance_timed_out(started, Instant::now()) {
            match cancel_planned_update_request(&paths.request, &published) {
                Ok(true) => {
                    return Err(ManagedAgentUpdateDrainError::before_acceptance(
                        "The agent did not accept the update request in time. Its existing process is still running.",
                    ));
                }
                Ok(false) => accepted = true,
                Err(error) => {
                    return Err(ManagedAgentUpdateDrainError::after_acceptance(format!(
                        "The agent update request could not be cancelled safely: {error}"
                    )));
                }
            }
        }
        tokio::time::sleep(DRAIN_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_or_waking_agent_can_begin_handoff() {
        assert!(update_can_begin(&ManagedAgentRuntimeLifecycle::Listening));
        assert!(update_can_begin(&ManagedAgentRuntimeLifecycle::Waking));
        assert!(update_can_begin(&ManagedAgentRuntimeLifecycle::Ready));
        assert!(update_can_begin(&ManagedAgentRuntimeLifecycle::Failed));
    }

    #[test]
    fn starting_or_stopped_agent_cannot_begin_handoff() {
        assert!(!update_can_begin(&ManagedAgentRuntimeLifecycle::Starting));
        assert!(!update_can_begin(&ManagedAgentRuntimeLifecycle::Stopped));
    }

    #[test]
    fn accepted_errors_are_never_reported_as_rollback_safe() {
        assert!(ManagedAgentUpdateDrainError::before_acceptance("before").rollback_safe);
        assert!(!ManagedAgentUpdateDrainError::after_acceptance("after").rollback_safe);
    }

    #[test]
    fn request_cancellation_begins_at_the_exact_fifteen_second_boundary() {
        let started = Instant::now();
        assert_eq!(REQUEST_ACCEPT_TIMEOUT, Duration::from_secs(15));
        assert!(!request_acceptance_timed_out(
            started,
            started + REQUEST_ACCEPT_TIMEOUT - Duration::from_nanos(1)
        ));
        assert!(request_acceptance_timed_out(
            started,
            started + REQUEST_ACCEPT_TIMEOUT
        ));
    }

    #[test]
    fn only_a_clean_planned_exit_can_complete_a_handoff() {
        let success = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .arg("--help")
            .status()
            .expect("run clean child");
        assert!(handoff_exit_is_clean(&success));

        #[cfg(unix)]
        {
            let failure = std::process::Command::new("/bin/sh")
                .args(["-c", "exit 17"])
                .status()
                .expect("run failing child");
            assert!(!handoff_exit_is_clean(&failure));
        }
        #[cfg(windows)]
        {
            let failure = std::process::Command::new("cmd")
                .args(["/C", "exit", "17"])
                .status()
                .expect("run failing child");
            assert!(!handoff_exit_is_clean(&failure));
        }
    }
}
