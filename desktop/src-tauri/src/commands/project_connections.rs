use std::collections::BTreeMap;

use tauri::{AppHandle, Manager};

use crate::{
    app_state::AppState,
    managed_agents::{
        find_managed_agent_mut, load_managed_agents, managed_agent_runtime_keys,
        project_connections::{
            self, CreateProjectConnectionRequest, ProjectConnection, ProjectConnectionImpact,
            UpdateProjectConnectionRequest,
        },
        save_managed_agents_for_operation, start_managed_agent_runtime_pair_authorized,
        stop_managed_agent_process, wait_for_managed_agent_runtime_started, AgentProjectScope,
    },
};

#[tauri::command]
pub fn list_project_connections(
    app: AppHandle,
    project_scope: AgentProjectScope,
) -> Result<Vec<ProjectConnection>, String> {
    project_connections::list_project_connections(&app, &project_scope)
}

#[tauri::command]
pub fn create_project_connection(
    app: AppHandle,
    input: CreateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    let state = app.state::<AppState>();
    let _mutation_lease = state
        .managed_agent_update_leases
        .try_acquire_global_mutation("project-connection-create")?;
    project_connections::create_project_connection(&app, input)
}

#[tauri::command]
pub async fn update_project_connection(
    app: AppHandle,
    input: UpdateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    let state = app.state::<AppState>();
    let mutation_lease = state
        .managed_agent_update_leases
        .try_acquire_global_mutation("project-connection-update")?;
    let operation_id = mutation_lease.operation_id();
    let connection_id = input.id.clone();
    let rollback = project_connections::snapshot_project_connection(&app, &connection_id)?;
    let impact = project_connections::project_connection_impact(&app, &connection_id)?;

    let (original_relays, stop_error) = {
        let _transition = state
            .managed_agent_runtime_transition
            .lock()
            .map_err(|error| error.to_string())?;
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut records = load_managed_agents(&app)?;
        let mut runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;
        let mut original_relays = BTreeMap::new();
        let mut stop_error = None;
        for agent in &impact.agents {
            let record = find_managed_agent_mut(&mut records, &agent.pubkey)?;
            let mut relays: Vec<_> = managed_agent_runtime_keys(&runtimes, &agent.pubkey)
                .into_iter()
                .map(|key| key.relay_url)
                .collect();
            relays.sort();
            original_relays.insert(agent.pubkey.clone(), relays);
            if let Err(error) = stop_managed_agent_process(&app, record, &mut runtimes) {
                stop_error = Some(format!(
                    "Buzz could not stop {} before changing its connection: {error}",
                    record.name
                ));
                break;
            }
        }
        save_managed_agents_for_operation(&app, &records, operation_id)?;
        (original_relays, stop_error)
    };

    if let Some(error) = stop_error {
        let restart_errors =
            restart_original_pairs_for_operation(&app, &original_relays, operation_id).await;
        if restart_errors.is_empty() {
            return Err(error);
        }
        return Err(format!(
            "{error} Buzz also could not restart the previous agents: {}",
            restart_errors
                .into_iter()
                .map(|(pubkey, error)| format!("{pubkey}: {error}"))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    let updated = match project_connections::update_project_connection(&app, input) {
        Ok(updated) => updated,
        Err(error) => {
            let restart_errors =
                restart_original_pairs_for_operation(&app, &original_relays, operation_id).await;
            if restart_errors.is_empty() {
                return Err(error);
            }
            return Err(format!(
                "{error} Buzz also could not restart the previous agents: {}",
                restart_errors
                    .into_iter()
                    .map(|(pubkey, error)| format!("{pubkey}: {error}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    };

    let restart_errors =
        restart_original_pairs_for_operation(&app, &original_relays, operation_id).await;
    if restart_errors.is_empty() {
        project_connections::finalize_project_connection_update(&rollback);
        return Ok(updated);
    }

    let cleanup_error = (|| -> Result<(), String> {
        let _transition = state
            .managed_agent_runtime_transition
            .lock()
            .map_err(|error| error.to_string())?;
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut records = load_managed_agents(&app)?;
        let mut runtimes = state
            .managed_agent_processes
            .lock()
            .map_err(|error| error.to_string())?;
        for agent in &impact.agents {
            let record = find_managed_agent_mut(&mut records, &agent.pubkey)?;
            stop_managed_agent_process(&app, record, &mut runtimes)?;
        }
        save_managed_agents_for_operation(&app, &records, operation_id)
    })()
    .err();
    let restore_error =
        project_connections::restore_project_connection(&app, &rollback, updated.generation).err();
    let recovery_restarts = if restore_error.is_none() {
        restart_original_pairs_for_operation(&app, &original_relays, operation_id).await
    } else {
        Vec::new()
    };

    let first_restart_error = restart_errors
        .first()
        .map(|(pubkey, error)| format!("{pubkey}: {error}"))
        .unwrap_or_else(|| "an updated agent did not become ready".to_string());
    let mut recovery_errors = Vec::new();
    if let Some(error) = cleanup_error {
        recovery_errors.push(format!("cleanup failed: {error}"));
    }
    if let Some(error) = restore_error {
        recovery_errors.push(format!("connection restore failed: {error}"));
    }
    recovery_errors.extend(
        recovery_restarts
            .into_iter()
            .map(|(pubkey, error)| format!("{pubkey} restart failed: {error}")),
    );
    if recovery_errors.is_empty() {
        Err(format!(
            "The updated connection could not start every affected agent ({first_restart_error}). Buzz restored the previous connection and restarted the agents."
        ))
    } else {
        Err(format!(
            "The updated connection could not start every affected agent ({first_restart_error}). Recovery needs attention: {}",
            recovery_errors.join("; ")
        ))
    }
}

#[tauri::command]
pub fn test_project_connection(
    app: AppHandle,
    connection_id: String,
) -> Result<ProjectConnection, String> {
    let state = app.state::<AppState>();
    let _mutation_lease = state
        .managed_agent_update_leases
        .try_acquire_global_mutation("project-connection-test")?;
    project_connections::test_project_connection(&app, &connection_id)
}

#[tauri::command]
pub fn get_project_connection_impact(
    app: AppHandle,
    connection_id: String,
) -> Result<ProjectConnectionImpact, String> {
    project_connections::project_connection_impact(&app, &connection_id)
}

#[tauri::command]
pub fn delete_project_connection(app: AppHandle, connection_id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _mutation_lease = state
        .managed_agent_update_leases
        .try_acquire_global_mutation("project-connection-delete")?;
    project_connections::delete_project_connection(&app, &connection_id)
}

async fn restart_original_pairs_for_operation(
    app: &AppHandle,
    original_relays: &BTreeMap<String, Vec<String>>,
    operation_id: &str,
) -> Vec<(String, String)> {
    // Keep the unleased compatibility helper reachable for its existing
    // non-transaction callers while this path uses the operation-aware form.
    let _unleased_restart = super::agent_template_updates::restart_original_pairs;
    let mut errors = Vec::new();
    for (pubkey, relays) in original_relays {
        for relay in relays {
            match start_managed_agent_runtime_pair_authorized(
                pubkey.clone(),
                relay.clone(),
                true,
                operation_id,
                app.clone(),
            ) {
                Ok(_) => {
                    if let Err(error) =
                        wait_for_managed_agent_runtime_started(app, pubkey, relay).await
                    {
                        errors.push((pubkey.clone(), error));
                    }
                }
                Err(error) => errors.push((pubkey.clone(), error)),
            }
        }
    }
    errors
}
