use super::*;

fn commit_create_with_credentials<StoreCredentials, SaveMetadata, CleanupCredentials>(
    operation: &operation_lease::ScopeOperationLease,
    has_credentials: bool,
    mut store_credentials: StoreCredentials,
    mut save_metadata: SaveMetadata,
    mut cleanup_credentials: CleanupCredentials,
) -> Result<(), String>
where
    StoreCredentials: FnMut() -> Result<(), String>,
    SaveMetadata: FnMut() -> Result<(), String>,
    CleanupCredentials: FnMut() -> Result<(), String>,
{
    if has_credentials {
        store_credentials()?;
    }
    let commit_result = operation.check_active().and_then(|()| save_metadata());
    if let Err(error) = commit_result {
        if has_credentials {
            if let Err(cleanup_error) = cleanup_credentials() {
                return Err(format!(
                    "{error} Buzz also could not remove the unreferenced credentials: {cleanup_error}"
                ));
            }
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn list_project_connections_at(
    app: &AppHandle,
    captured_scope: &CapturedProjectConnectionScope,
) -> Result<Vec<ProjectConnection>, String> {
    let _guard = lock_project_connections();
    let mut connections: Vec<_> = load_store_unlocked(app, captured_scope)?
        .connections
        .into_iter()
        .filter(|connection| connection.project_scope == captured_scope.project)
        .map(health_for_display)
        .map(ProjectConnection::from)
        .collect();
    connections.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    validate_captured_scope_for_app(app, captured_scope)?;
    Ok(connections)
}

pub(crate) fn create_project_connection_at(
    app: &AppHandle,
    captured_scope: &CapturedProjectConnectionScope,
    mut input: CreateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    input.project_scope = captured_scope.project.clone();
    validate_connection_input(
        &input.name,
        &input.provider,
        &input.command,
        &input.args,
        &input.env,
    )?;
    if !input.execution_acknowledged {
        return Err("Review and acknowledge this local program before saving.".to_string());
    }
    let (command, executable_sha256) = canonical_connection_command(&input.command)?;
    let executable_sha256 = approved_execution_sha256(&executable_sha256, &input.args)?;
    let _guard = lock_project_connections();
    let mut store = load_store_unlocked(app, captured_scope)?;
    if store.connections.len() >= MAX_WORKSPACE_CONNECTIONS {
        return Err("Buzz has reached the workspace connection limit.".to_string());
    }
    if project_connection_count(&store, &input.project_scope) >= MAX_PROJECT_CONNECTIONS {
        return Err("Buzz has reached the Project connection limit.".to_string());
    }
    let id = Uuid::new_v4().simple().to_string();
    let now = now_iso();
    let credential_generation = next_generation();
    let connection = StoredProjectConnection {
        id: id.clone(),
        project_scope: input.project_scope.clone(),
        name: input.name.trim().to_string(),
        provider: input.provider.trim().to_string(),
        capability_ids: Vec::new(),
        command,
        args: input.args,
        env_keys: input.env.keys().cloned().collect(),
        discovered_tools: Vec::new(),
        health: ProjectConnectionHealth::default(),
        executable_sha256,
        generation: next_generation(),
        credential_generation: credential_generation.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    store.connections.push(connection.clone());
    validate_new_store_size(&store)?;
    let has_credentials = !input.env.is_empty();
    let credential_target = if has_credentials {
        Some(capture_credential_target(
            app,
            captured_scope,
            &id,
            &credential_generation,
        )?)
    } else {
        None
    };
    if has_credentials {
        credential_journal::begin(
            app,
            captured_scope,
            &id,
            vec![credential_generation.clone()],
        )?;
    }
    commit_create_with_credentials(
        &captured_scope.operation,
        has_credentials,
        || {
            credential_target
                .as_ref()
                .map_or(Ok(()), |target| store_secrets_at_target(target, &input.env))
        },
        || save_store_unlocked(app, captured_scope, &store),
        || {
            credential_target
                .as_ref()
                .map_or(Ok(()), delete_secrets_at_target)
        },
    )?;
    if has_credentials {
        if let Err(error) = credential_journal::complete(app, captured_scope) {
            eprintln!(
                "buzz-desktop: Project connection credential recovery marker remains after create: {error}"
            );
        }
    }
    Ok(connection.into())
}

pub(crate) fn update_project_connection_at(
    app: &AppHandle,
    captured_scope: &CapturedProjectConnectionScope,
    mut input: UpdateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    input.project_scope = captured_scope.project.clone();
    for key in &input.remove_env_keys {
        if !super::super::is_well_formed_env_key(key) {
            return Err("A secret name is invalid.".to_string());
        }
    }
    let (command, executable_sha256) = canonical_connection_command(&input.command)?;
    let executable_sha256 = approved_execution_sha256(&executable_sha256, &input.args)?;
    probe::with_project_connection_probe_excluded(|| {
        let _guard = lock_project_connections();
        let mut store = load_store_unlocked(app, captured_scope)?;
        let previous = find_connection(&store, &input.project_scope, &input.id)?.clone();
        let previous_secrets = load_secrets(app, captured_scope, &previous)?;
        let mut next_secrets = previous_secrets.clone();
        for key in &input.remove_env_keys {
            next_secrets.remove(key);
        }
        next_secrets.extend(input.env);
        validate_connection_input(
            &input.name,
            &input.provider,
            &command,
            &input.args,
            &next_secrets,
        )?;
        let execution_changed = previous.command != command
            || previous.executable_sha256 != executable_sha256
            || previous.args != input.args
            || previous_secrets != next_secrets;
        if execution_changed && !input.execution_acknowledged {
            return Err(
                "Review and acknowledge the changed program, arguments, and credentials before saving."
                    .to_string(),
            );
        }
        let index = find_connection_index(&store, &input.project_scope, &input.id)
            .ok_or_else(|| "This connection no longer exists.".to_string())?;
        let mut updated = previous.clone();
        updated.name = input.name.trim().to_string();
        updated.provider = input.provider.trim().to_string();
        updated.command = command;
        updated.executable_sha256 = executable_sha256;
        updated.args = input.args;
        updated.env_keys = next_secrets.keys().cloned().collect();
        updated.generation = next_generation();
        updated.updated_at = now_iso();
        if execution_changed {
            updated.capability_ids.clear();
            updated.discovered_tools.clear();
            updated.health = ProjectConnectionHealth::default();
        }
        let mut size_candidate = store.clone();
        size_candidate.connections[index] = updated.clone();
        validate_updated_store_size(&store, &size_candidate)?;

        let secrets_changed = previous_secrets != next_secrets;
        let new_credential_target = if secrets_changed {
            updated.credential_generation = next_generation();
            Some(capture_credential_target(
                app,
                captured_scope,
                &input.id,
                &updated.credential_generation,
            )?)
        } else {
            None
        };
        if secrets_changed {
            credential_journal::begin(
                app,
                captured_scope,
                &input.id,
                vec![
                    previous.credential_generation.clone(),
                    updated.credential_generation.clone(),
                ],
            )?;
        }
        let result = commit_update(
            &mut store,
            UpdateTransaction {
                index,
                previous: &previous,
                updated: &updated,
                secrets_changed,
            },
            || {
                new_credential_target.as_ref().map_or(Ok(()), |target| {
                    store_secrets_at_target(target, &next_secrets)
                })
            },
            |candidate| save_store_unlocked(app, captured_scope, candidate),
            |generation| {
                if let Some(target) = new_credential_target
                    .as_ref()
                    .filter(|target| target.credential_generation == generation)
                {
                    delete_secrets_at_target(target)
                } else {
                    delete_secrets(app, captured_scope, &input.id, generation)
                }
            },
        );
        if result.is_ok() && secrets_changed {
            if let Err(error) = credential_journal::complete(app, captured_scope) {
                eprintln!(
                    "buzz-desktop: Project connection credential recovery marker remains after update: {error}"
                );
            }
        }
        result?;
        Ok(updated.into())
    })
}

pub(crate) fn delete_project_connection_at(
    app: &AppHandle,
    captured_scope: &CapturedProjectConnectionScope,
    connection_id: &str,
) -> Result<(), String> {
    let project_scope = captured_scope.project.clone();
    probe::with_project_connection_probe_excluded(|| {
        let _guard = lock_project_connections();
        let mut store = load_store_unlocked(app, captured_scope)?;
        let index = find_connection_index(&store, &project_scope, connection_id)
            .ok_or_else(|| "This connection no longer exists in this Project.".to_string())?;
        let removed = store.connections[index].clone();
        if !removed.env_keys.is_empty() {
            credential_journal::begin(
                app,
                captured_scope,
                connection_id,
                vec![removed.credential_generation.clone()],
            )?;
        }
        let result = commit_delete(
            &mut store,
            index,
            |candidate| save_store_unlocked(app, captured_scope, candidate),
            |generation| delete_secrets(app, captured_scope, connection_id, generation),
        );
        if result.is_ok() && !removed.env_keys.is_empty() {
            if let Err(error) = credential_journal::complete(app, captured_scope) {
                eprintln!(
                    "buzz-desktop: Project connection credential recovery marker remains after delete: {error}"
                );
            }
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        },
        thread,
    };

    use super::*;

    fn project(project_name: &str) -> ProjectConnectionScope {
        ProjectConnectionScope {
            relay_url: "ws://127.0.0.1:3000".to_string(),
            operator_pubkey: "a".repeat(64),
            project_address: format!("30621:{}:{project_name}", "a".repeat(64)),
        }
    }

    fn stored_connection(project: ProjectConnectionScope) -> StoredProjectConnection {
        StoredProjectConnection {
            id: "c".repeat(32),
            project_scope: project,
            name: "Test".to_string(),
            provider: "Fixture".to_string(),
            capability_ids: Vec::new(),
            command: "/usr/bin/true".to_string(),
            args: Vec::new(),
            env_keys: vec!["TOKEN".to_string()],
            discovered_tools: Vec::new(),
            health: ProjectConnectionHealth::default(),
            executable_sha256: "e".repeat(64),
            generation: "f".repeat(32),
            credential_generation: "d".repeat(32),
            created_at: "2026-08-05T00:00:00Z".to_string(),
            updated_at: "2026-08-05T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn cancellation_after_credential_store_runs_exact_cleanup_before_drain() {
        let coordinator = Arc::new(ScopeOperationCoordinator::default());
        let operation = coordinator.acquire(41).unwrap();
        let cancellation_observer = coordinator.acquire(41).unwrap();
        let target = test_credential_target(
            &"1".repeat(64),
            project("current"),
            &"c".repeat(32),
            &"d".repeat(32),
        );
        let other_scope_target = test_credential_target(
            &"2".repeat(64),
            project("other"),
            &"c".repeat(32),
            &"d".repeat(32),
        );
        let backend = Arc::new(Mutex::new(HashSet::from([other_scope_target.clone()])));
        let metadata_saved = Arc::new(AtomicBool::new(false));
        let (stored_tx, stored_rx) = mpsc::sync_channel(0);
        let (release_store_tx, release_store_rx) = mpsc::sync_channel(0);
        let (cleanup_tx, cleanup_rx) = mpsc::sync_channel(0);
        let (release_cleanup_tx, release_cleanup_rx) = mpsc::sync_channel(0);

        let operation_target = target.clone();
        let operation_backend = Arc::clone(&backend);
        let operation_metadata_saved = Arc::clone(&metadata_saved);
        let operation_thread = thread::spawn(move || {
            commit_create_with_credentials(
                &operation,
                true,
                || {
                    operation_backend
                        .lock()
                        .unwrap()
                        .insert(operation_target.clone());
                    stored_tx.send(()).unwrap();
                    release_store_rx.recv().unwrap();
                    Ok(())
                },
                || {
                    operation_metadata_saved.store(true, Ordering::Release);
                    Ok(())
                },
                || {
                    cleanup_tx.send(()).unwrap();
                    release_cleanup_rx.recv().unwrap();
                    operation_backend.lock().unwrap().remove(&operation_target);
                    Ok(())
                },
            )
        });

        stored_rx.recv().unwrap();
        let (drained_tx, drained_rx) = mpsc::sync_channel(0);
        let drain_coordinator = Arc::clone(&coordinator);
        let drain_thread = thread::spawn(move || {
            drain_coordinator.cancel_and_drain(41);
            drained_tx.send(()).unwrap();
        });
        while !cancellation_observer.is_cancelled() {
            thread::yield_now();
        }

        release_store_tx.send(()).unwrap();
        cleanup_rx.recv().unwrap();
        assert!(!metadata_saved.load(Ordering::Acquire));
        assert_eq!(drained_rx.try_recv(), Err(mpsc::TryRecvError::Empty));

        release_cleanup_tx.send(()).unwrap();
        let error = operation_thread.join().unwrap().unwrap_err();
        assert!(error.contains("workspace changed"));
        drop(cancellation_observer);
        drained_rx.recv().unwrap();
        drain_thread.join().unwrap();

        let final_backend = backend.lock().unwrap();
        assert!(!final_backend.contains(&target));
        assert!(final_backend.contains(&other_scope_target));
    }

    #[test]
    fn cancellation_during_exact_delete_waits_without_touching_another_scope() {
        let coordinator = Arc::new(ScopeOperationCoordinator::default());
        let operation = coordinator.acquire(52).unwrap();
        let cancellation_observer = coordinator.acquire(52).unwrap();
        let target = test_credential_target(
            &"1".repeat(64),
            project("current"),
            &"c".repeat(32),
            &"d".repeat(32),
        );
        let other_scope_target = test_credential_target(
            &"2".repeat(64),
            project("other"),
            &"c".repeat(32),
            &"d".repeat(32),
        );
        let backend = Arc::new(Mutex::new(HashSet::from([
            target.clone(),
            other_scope_target.clone(),
        ])));
        let (delete_started_tx, delete_started_rx) = mpsc::sync_channel(0);
        let (release_delete_tx, release_delete_rx) = mpsc::sync_channel(0);

        let operation_target = target.clone();
        let operation_backend = Arc::clone(&backend);
        let operation_thread = thread::spawn(move || {
            let mut store = ProjectConnectionStore {
                version: CONNECTION_STORE_VERSION,
                connections: vec![stored_connection(project("current"))],
            };
            commit_delete(
                &mut store,
                0,
                |_| operation.check_active(),
                |_| {
                    delete_started_tx.send(()).unwrap();
                    release_delete_rx.recv().unwrap();
                    operation_backend.lock().unwrap().remove(&operation_target);
                    Ok(())
                },
            )?;
            Ok::<ProjectConnectionStore, String>(store)
        });

        delete_started_rx.recv().unwrap();
        let (drained_tx, drained_rx) = mpsc::sync_channel(0);
        let drain_coordinator = Arc::clone(&coordinator);
        let drain_thread = thread::spawn(move || {
            drain_coordinator.cancel_and_drain(52);
            drained_tx.send(()).unwrap();
        });
        while !cancellation_observer.is_cancelled() {
            thread::yield_now();
        }
        assert_eq!(drained_rx.try_recv(), Err(mpsc::TryRecvError::Empty));

        release_delete_tx.send(()).unwrap();
        let store = operation_thread.join().unwrap().unwrap();
        assert!(store.connections.is_empty());
        drop(cancellation_observer);
        drained_rx.recv().unwrap();
        drain_thread.join().unwrap();

        let final_backend = backend.lock().unwrap();
        assert!(!final_backend.contains(&target));
        assert!(final_backend.contains(&other_scope_target));
    }
}
