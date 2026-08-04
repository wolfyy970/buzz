use super::*;

fn scope() -> ProjectConnectionScope {
    ProjectConnectionScope {
        relay_url: "ws://127.0.0.1:3000".to_string(),
        operator_pubkey: "b".repeat(64),
        project_address: format!("30621:{}:portable-agents", "a".repeat(64)),
    }
}

pub(super) fn stored_connection() -> StoredProjectConnection {
    StoredProjectConnection {
        id: "c".repeat(32),
        project_scope: scope(),
        name: "Analytics".to_string(),
        provider: "Local test".to_string(),
        capability_ids: vec!["mcp.tool.run_report".to_string()],
        command: "/usr/bin/true".to_string(),
        args: Vec::new(),
        env_keys: vec!["API_TOKEN".to_string()],
        discovered_tools: vec!["run_report".to_string()],
        health: ProjectConnectionHealth {
            status: ProjectConnectionHealthStatus::Ready,
            last_verified_at: Some(now_iso()),
            detail: None,
        },
        executable_sha256: "d".repeat(64),
        generation: "e".repeat(32),
        credential_generation: "f".repeat(32),
        created_at: now_iso(),
        updated_at: now_iso(),
    }
}

fn captured_scope(base: &std::path::Path, generation: u64) -> CapturedProjectConnectionScope {
    let coordinator = operation_lease::ScopeOperationCoordinator::default();
    captured_scope_with_coordinator(base, generation, &coordinator)
}

fn captured_scope_with_coordinator(
    base: &std::path::Path,
    generation: u64,
    coordinator: &operation_lease::ScopeOperationCoordinator,
) -> CapturedProjectConnectionScope {
    let project = canonical_project_scope(&scope()).unwrap();
    let operation = coordinator.acquire(generation).unwrap();
    let captured = CapturedProjectConnectionScope {
        workspace: super::super::scope::WorkspaceAgentScope::new(
            project.relay_url.clone(),
            project.operator_pubkey.clone(),
            base,
            generation,
        ),
        project,
        operation,
    };
    fs::create_dir_all(&captured.workspace.definitions_dir).unwrap();
    fs::write(
        captured.workspace.definitions_dir.join("_manifest.json"),
        serde_json::to_vec(&super::super::scope_init::ScopeManifest {
            scope_id: captured.workspace.scope_id.clone(),
            init_kind: super::super::scope_init::ScopeInitKind::FreshNoLegacy,
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(captured.workspace.definitions_dir.join("_ready"), b"v1\n").unwrap();
    captured
}

#[test]
fn scope_drain_does_not_deadlock_a_save_at_the_commit_fence() {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    let _generation_guard = super::super::scope::SCOPE_GENERATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let generation = super::super::scope::next_scope_generation();
    let coordinator = Arc::new(operation_lease::ScopeOperationCoordinator::default());
    let captured = Arc::new(captured_scope_with_coordinator(
        dir.path(),
        generation,
        &coordinator,
    ));
    let (commit_started_tx, commit_started_rx) = mpsc::channel();
    let (finish_commit_tx, finish_commit_rx) = mpsc::channel();

    let saver = {
        let captured = Arc::clone(&captured);
        std::thread::spawn(move || {
            with_scope_commit_fence(&captured, || {
                commit_started_tx.send(()).unwrap();
                finish_commit_rx.recv().unwrap();
                Ok(())
            })
        })
    };

    commit_started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("leased save must reach its commit without another scope lock");
    let drainer = {
        let coordinator = Arc::clone(&coordinator);
        std::thread::spawn(move || coordinator.cancel_and_drain(generation))
    };
    while !captured.operation.is_cancelled() {
        std::thread::yield_now();
    }
    finish_commit_tx.send(()).unwrap();
    assert!(saver.join().unwrap().is_ok());
    drop(captured);
    drainer.join().unwrap();
}

#[test]
fn cancellation_before_commit_fence_never_runs_the_write() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let _generation_guard = super::super::scope::SCOPE_GENERATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let generation = super::super::scope::next_scope_generation();
    let coordinator = Arc::new(operation_lease::ScopeOperationCoordinator::default());
    let captured = captured_scope_with_coordinator(dir.path(), generation, &coordinator);
    let drainer = {
        let coordinator = Arc::clone(&coordinator);
        std::thread::spawn(move || coordinator.cancel_and_drain(generation))
    };
    while !captured.operation.is_cancelled() {
        std::thread::yield_now();
    }

    let wrote = AtomicBool::new(false);
    assert!(with_scope_commit_fence(&captured, || {
        wrote.store(true, Ordering::Release);
        Ok(())
    })
    .is_err());
    assert!(!wrote.load(Ordering::Acquire));

    drop(captured);
    drainer.join().unwrap();
}

#[test]
fn project_scope_requires_canonical_relay_identity_and_coordinate() {
    assert_eq!(canonical_project_scope(&scope()).unwrap(), scope());
    let mut localhost = scope();
    localhost.relay_url = "ws://localhost:3000".to_string();
    assert_eq!(canonical_project_scope(&localhost).unwrap(), scope());
    let mut invalid = scope();
    invalid.project_address = "local-project-id".to_string();
    assert!(canonical_project_scope(&invalid).is_err());
    let mut invalid = scope();
    invalid.operator_pubkey = "not-a-key".to_string();
    assert!(canonical_project_scope(&invalid).is_err());

    let mut legacy = scope();
    legacy.project_address = format!("30617:{}:portable-agents", "a".repeat(64));
    assert_eq!(canonical_project_scope(&legacy).unwrap(), legacy);
}

#[test]
fn connection_scope_uses_the_workspace_agent_scope_identity() {
    let scope = canonical_project_scope(&scope()).unwrap();
    let workspace = super::super::scope::WorkspaceAgentScope::new(
        scope.relay_url.clone(),
        scope.operator_pubkey.clone(),
        std::path::Path::new("/tmp/buzz-agent-scope"),
        7,
    );

    assert_eq!(workspace_scope_id(&scope), workspace.scope_id);
}

#[test]
fn connection_store_reads_the_pre_project_address_field() {
    let connection = stored_connection();
    let mut json = serde_json::to_value(ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections: vec![connection.clone()],
    })
    .unwrap();
    let scope = &mut json["connections"][0]["projectScope"];
    scope["repoAddress"] = scope["projectAddress"].take();
    scope.as_object_mut().unwrap().remove("projectAddress");

    let restored: ProjectConnectionStore = serde_json::from_value(json).unwrap();
    assert_eq!(restored.connections, [connection]);
}

#[test]
fn public_projection_omits_secret_values_and_internal_generation() {
    let public = ProjectConnection::from(stored_connection());
    let json = serde_json::to_value(public).unwrap();
    assert_eq!(json["envKeys"], serde_json::json!(["API_TOKEN"]));
    assert!(json.get("generation").is_none());
    assert!(!json.to_string().contains("private-generation"));
    assert!(!json.to_string().contains("secret-value"));
}

#[test]
fn connection_input_rejects_reserved_empty_and_oversized_secrets() {
    let valid = BTreeMap::from([("API_TOKEN".to_string(), "value".to_string())]);
    assert!(validate_connection_input("Analytics", "Local", "/bin/true", &[], &valid).is_ok());
    let reserved = BTreeMap::from([("BUZZ_PRIVATE_KEY".to_string(), "value".to_string())]);
    assert!(validate_connection_input("Analytics", "Local", "/bin/true", &[], &reserved).is_err());
    let empty = BTreeMap::from([("API_TOKEN".to_string(), String::new())]);
    assert!(validate_connection_input("Analytics", "Local", "/bin/true", &[], &empty).is_err());
    let oversized = BTreeMap::from([("API_TOKEN".to_string(), "x".repeat(MAX_SECRET_BYTES + 1))]);
    assert!(validate_connection_input("Analytics", "Local", "/bin/true", &[], &oversized).is_err());
}

#[test]
fn connection_input_rejects_case_collisions_without_echoing_pasted_secrets() {
    let collision = BTreeMap::from([
        ("API_TOKEN".to_string(), "one".to_string()),
        ("api_token".to_string(), "two".to_string()),
    ]);
    assert!(validate_connection_input("Analytics", "Local", "/bin/true", &[], &collision).is_err());

    let pasted = BTreeMap::from([(
        "ANTHROPIC_API_KEY=sk-must-not-echo".to_string(),
        "ignored".to_string(),
    )]);
    let error =
        validate_connection_input("Analytics", "Local", "/bin/true", &[], &pasted).unwrap_err();
    assert!(!error.contains("sk-must-not-echo"));
    assert!(error.contains("ANTHROPIC_API_KEY"));
}

#[test]
fn verified_secret_write_keeps_the_saved_generation() {
    use std::cell::Cell;

    let deleted = Cell::new(false);
    assert!(store_verified_secret(
        || Ok(()),
        || Ok(true),
        || {
            deleted.set(true);
            Ok(())
        },
    )
    .is_ok());
    assert!(!deleted.get());
}

#[test]
fn failed_secret_verification_removes_the_saved_generation() {
    use std::cell::Cell;

    let deleted = Cell::new(false);
    let error = store_verified_secret(
        || Ok(()),
        || Ok(false),
        || {
            deleted.set(true);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error, "Buzz could not verify the saved credentials.");
    assert!(deleted.get());
}

#[test]
fn secret_verification_error_removes_the_saved_generation() {
    use std::cell::Cell;

    let deleted = Cell::new(false);
    let error = store_verified_secret(
        || Ok(()),
        || Err("backend detail".to_string()),
        || {
            deleted.set(true);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error, "Buzz could not verify the saved credentials.");
    assert!(deleted.get());
}

#[test]
fn secret_write_error_removes_a_possible_partial_generation() {
    use std::cell::Cell;

    let verified = Cell::new(false);
    let deleted = Cell::new(false);
    let error = store_verified_secret(
        || Err("backend detail".to_string()),
        || {
            verified.set(true);
            Ok(true)
        },
        || {
            deleted.set(true);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(
        error,
        "Buzz could not save these credentials in the system keyring."
    );
    assert!(!verified.get());
    assert!(deleted.get());
}

#[test]
fn failed_secret_cleanup_is_reported_without_backend_details() {
    let error = store_verified_secret(
        || Ok(()),
        || Ok(false),
        || Err("backend detail".to_string()),
    )
    .unwrap_err();
    assert_eq!(
        error,
        "Buzz could not verify the saved credentials. \
         Buzz also could not remove the unverified credentials."
    );
    assert!(!error.contains("backend detail"));
}

#[test]
fn stale_ready_connection_is_presented_as_check_needed() {
    let mut connection = stored_connection();
    connection.health.last_verified_at = Some("2020-01-01T00:00:00Z".to_string());
    assert_eq!(
        health_for_display(connection).health.status,
        ProjectConnectionHealthStatus::CheckNeeded
    );
}

#[test]
fn stored_connection_rejects_invalid_ids_generations_and_fingerprints() {
    let mut connection = stored_connection();
    assert!(validate_stored_connection(&connection).is_ok());
    connection.id = "../outside".to_string();
    assert!(validate_stored_connection(&connection).is_err());

    let mut connection = stored_connection();
    connection.credential_generation = "not-a-generation".to_string();
    assert!(validate_stored_connection(&connection).is_err());

    let mut connection = stored_connection();
    connection.executable_sha256 = "not-a-fingerprint".to_string();
    assert!(validate_stored_connection(&connection).is_err());
}

#[test]
fn stored_connection_revalidates_all_runtime_facing_fields() {
    let mut connection = stored_connection();
    connection.name = "spoofed\nname".to_string();
    assert!(validate_stored_connection(&connection).is_err());

    let mut connection = stored_connection();
    connection.args = vec!["x".repeat(MAX_ARG_BYTES + 1)];
    assert!(validate_stored_connection(&connection).is_err());

    let mut connection = stored_connection();
    connection.env_keys = vec!["API_TOKEN".to_string(), "api_token".to_string()];
    assert!(validate_stored_connection(&connection).is_err());

    let mut connection = stored_connection();
    connection.discovered_tools = vec!["unsupported.dotted".to_string()];
    connection.capability_ids = vec!["mcp.tool.unsupported.dotted".to_string()];
    assert!(validate_stored_connection(&connection).is_err());

    let mut connection = stored_connection();
    connection.capability_ids = vec!["mcp.tool.different".to_string()];
    assert!(validate_stored_connection(&connection).is_err());
}

#[test]
fn connection_store_size_is_bounded_before_deserialization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.json");
    fs::write(&path, vec![b' '; MAX_CONNECTION_STORE_BYTES + 1]).unwrap();
    let error = read_bounded_file(&path, MAX_CONNECTION_STORE_BYTES).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn connection_lookup_cannot_cross_project_boundaries() {
    let connection = stored_connection();
    let store = ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections: vec![connection.clone()],
    };
    assert!(find_connection(&store, &connection.project_scope, &connection.id).is_ok());

    let mut other_project = connection.project_scope;
    other_project.project_address = format!("30621:{}:other-project", "a".repeat(64));
    assert!(find_connection(&store, &other_project, &connection.id).is_err());
}

#[test]
fn connection_store_rejects_duplicate_ids_across_projects() {
    let first = stored_connection();
    let mut second = first.clone();
    second.project_scope.project_address = format!("30621:{}:other-project", "a".repeat(64));
    let store = ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections: vec![first, second],
    };
    let dir = tempfile::tempdir().unwrap();
    // Active workspace scopes are created by `next_scope_generation`, so
    // generation zero is not a production state.
    let captured = captured_scope(dir.path(), 1);

    assert_eq!(
        validate_store_for_scope(&store, &captured).unwrap_err(),
        "Project connection metadata contains duplicate identities."
    );
}

#[test]
fn stale_workspace_generation_cannot_create_connection_paths() {
    let _generation_guard = super::super::scope::SCOPE_GENERATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let generation = super::super::scope::next_scope_generation();
    let captured = captured_scope(dir.path(), generation);
    let path = captured
        .workspace
        .definitions_dir
        .join("project-connections");

    super::super::scope::next_scope_generation();
    let error = workspace_connection_dir(&captured).unwrap_err();

    assert!(error.contains("stale scope"));
    assert!(!path.exists(), "stale operation must not create any path");
}

#[test]
fn workspace_and_project_connection_limits_are_distinct() {
    let target_scope = scope();
    let mut other = stored_connection();
    other.project_scope.project_address = format!("30621:{}:other-project", "a".repeat(64));
    let store = ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections: vec![other; MAX_PROJECT_CONNECTIONS],
    };

    assert_eq!(project_connection_count(&store, &target_scope), 0);
    assert_eq!(store.connections.len(), MAX_PROJECT_CONNECTIONS);
    assert!(store.connections.len() < MAX_WORKSPACE_CONNECTIONS);
}

#[test]
fn store_accepts_legacy_per_project_counts_but_keeps_workspace_limit() {
    let dir = tempfile::tempdir().unwrap();
    let captured = captured_scope(dir.path(), 1);
    let mut connections = Vec::new();
    for project_index in 0..4 {
        for connection_index in 0..MAX_PROJECT_CONNECTIONS {
            let mut connection = stored_connection();
            connection.id = format!("{:032x}", project_index * 1000 + connection_index);
            connection.project_scope.project_address =
                format!("30621:{}:project-{project_index}", "a".repeat(64));
            connections.push(connection);
        }
    }
    let full = ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections,
    };
    assert_eq!(full.connections.len(), MAX_WORKSPACE_CONNECTIONS);
    validate_store_for_scope(&full, &captured).unwrap();

    let mut too_many_for_project = full.clone();
    let mut extra = stored_connection();
    extra.id = format!("{:032x}", 99_999);
    extra.project_scope.project_address = format!("30621:{}:project-0", "a".repeat(64));
    too_many_for_project.connections[0] = extra.clone();
    too_many_for_project.connections.push(extra);
    assert!(validate_store_for_scope(&too_many_for_project, &captured)
        .unwrap_err()
        .contains("workspace limit"));

    let mut per_project = ProjectConnectionStore::default();
    for index in 0..=MAX_PROJECT_CONNECTIONS {
        let mut connection = stored_connection();
        connection.id = format!("{index:032x}");
        per_project.connections.push(connection);
    }
    validate_store_for_scope(&per_project, &captured).unwrap();
    assert!(project_connection_count(&per_project, &scope()) > MAX_PROJECT_CONNECTIONS);
}

#[test]
fn legacy_store_over_new_size_limit_remains_readable_but_cannot_grow() {
    let mut first = stored_connection();
    first.env_keys.clear();
    first.args = vec!["x".repeat(MAX_ARG_BYTES); MAX_ARGS];
    let mut second = first.clone();
    second.id = "d".repeat(32);
    let store = ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections: vec![first, second],
    };

    let serialized = serialize_store_bounded(&store).unwrap();
    assert!(serialized.len() as u64 > MAX_CONNECTION_STORE_BYTES);
    assert_eq!(
        validate_new_store_size(&store).unwrap_err(),
        "Project connection store exceeds its size limit."
    );
    validate_updated_store_size(&store, &store).unwrap();

    let mut larger = store.clone();
    larger.connections[0].name.push('x');
    assert!(validate_updated_store_size(&store, &larger)
        .unwrap_err()
        .contains("legacy Project connection store larger"));

    let mut smaller = store.clone();
    smaller.connections.pop();
    validate_updated_store_size(&store, &smaller).unwrap();
}

#[test]
fn escaped_secret_serialization_and_reader_share_the_exact_limit() {
    let key = "TOKEN".to_string();
    let value = "\u{0001}".repeat(MAX_SECRET_BYTES - key.len());
    let env = BTreeMap::from([(key.clone(), value)]);
    validate_connection_input("Test", "Fixture", "/usr/bin/true", &[], &env).unwrap();
    let serialized = serialize_secrets(&env).unwrap();
    assert!(serialized.len() as u64 <= MAX_SECRET_FILE_BYTES);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.json");
    atomic_write_json_restricted(&path, &serialized).unwrap();
    assert_eq!(
        read_bounded_owner_file(
            &path,
            MAX_SECRET_FILE_BYTES,
            "Project connection credentials",
        )
        .unwrap()
        .unwrap(),
        serialized,
    );

    let oversized = BTreeMap::from([(
        key.clone(),
        "\u{0001}".repeat(MAX_SECRET_BYTES - key.len() + 1),
    )]);
    assert!(
        validate_connection_input("Test", "Fixture", "/usr/bin/true", &[], &oversized,)
            .unwrap_err()
            .contains("size limit")
    );
}

#[test]
fn legacy_connection_store_migration_is_atomic_and_idempotent() {
    let _generation_guard = super::super::scope::SCOPE_GENERATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let captured = captured_scope(dir.path(), super::super::scope::next_scope_generation());
    let legacy = dir
        .path()
        .join("project-connections")
        .join(&captured.workspace.scope_id);
    ensure_owner_only_directory(legacy.parent().unwrap()).unwrap();
    ensure_owner_only_directory(&legacy).unwrap();
    let store = ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections: vec![stored_connection()],
    };
    atomic_write_json_restricted(
        &legacy.join("connections.json"),
        &serde_json::to_vec_pretty(&store).unwrap(),
    )
    .unwrap();

    let target = workspace_connection_dir(&captured).unwrap();
    assert!(!legacy.exists());
    assert!(target.join("connections.json").is_file());
    assert_eq!(workspace_connection_dir(&captured).unwrap(), target);
}

#[test]
fn migration_preserves_an_old_valid_store_larger_than_one_megabyte() {
    let _generation_guard = super::super::scope::SCOPE_GENERATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let captured = captured_scope(dir.path(), super::super::scope::next_scope_generation());
    let legacy = dir
        .path()
        .join("project-connections")
        .join(&captured.workspace.scope_id);
    ensure_owner_only_directory(legacy.parent().unwrap()).unwrap();
    ensure_owner_only_directory(&legacy).unwrap();
    let mut first = stored_connection();
    first.env_keys.clear();
    first.args = vec!["x".repeat(MAX_ARG_BYTES); MAX_ARGS];
    let mut second = first.clone();
    second.id = "d".repeat(32);
    let store = ProjectConnectionStore {
        version: CONNECTION_STORE_VERSION,
        connections: vec![first, second],
    };
    let bytes = serde_json::to_vec_pretty(&store).unwrap();
    assert!(bytes.len() as u64 > MAX_CONNECTION_STORE_BYTES);
    atomic_write_json_restricted(&legacy.join("connections.json"), &bytes).unwrap();

    let target = workspace_connection_dir(&captured).unwrap();
    let migrated = read_bounded_owner_file(
        &target.join("connections.json"),
        MAX_LEGACY_CONNECTION_STORE_BYTES,
        "Project connection store",
    )
    .unwrap()
    .unwrap();
    assert_eq!(migrated, bytes);
    let migrated_store: ProjectConnectionStore = serde_json::from_slice(&migrated).unwrap();
    validate_store_for_scope(&migrated_store, &captured).unwrap();
}

#[test]
fn migration_retry_replaces_only_an_empty_uncommitted_target() {
    let _generation_guard = super::super::scope::SCOPE_GENERATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let captured = captured_scope(dir.path(), super::super::scope::next_scope_generation());
    let legacy = dir
        .path()
        .join("project-connections")
        .join(&captured.workspace.scope_id);
    ensure_owner_only_directory(legacy.parent().unwrap()).unwrap();
    ensure_owner_only_directory(&legacy).unwrap();
    atomic_write_json_restricted(
        &legacy.join("connections.json"),
        &serde_json::to_vec_pretty(&ProjectConnectionStore {
            version: CONNECTION_STORE_VERSION,
            connections: vec![stored_connection()],
        })
        .unwrap(),
    )
    .unwrap();
    let target = captured
        .workspace
        .definitions_dir
        .join("project-connections");
    ensure_owner_only_directory(&target).unwrap();

    assert_eq!(workspace_connection_dir(&captured).unwrap(), target);
    assert!(!legacy.exists());
    assert!(target.join("connections.json").is_file());
}

#[test]
fn migration_conflict_preserves_both_stores_for_recovery() {
    let _generation_guard = super::super::scope::SCOPE_GENERATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let captured = captured_scope(dir.path(), super::super::scope::next_scope_generation());
    let legacy = dir
        .path()
        .join("project-connections")
        .join(&captured.workspace.scope_id);
    ensure_owner_only_directory(legacy.parent().unwrap()).unwrap();
    ensure_owner_only_directory(&legacy).unwrap();
    atomic_write_json_restricted(
        &legacy.join("connections.json"),
        &serde_json::to_vec_pretty(&ProjectConnectionStore {
            version: CONNECTION_STORE_VERSION,
            connections: vec![stored_connection()],
        })
        .unwrap(),
    )
    .unwrap();
    let target = captured
        .workspace
        .definitions_dir
        .join("project-connections");
    ensure_owner_only_directory(&target).unwrap();
    atomic_write_json_restricted(
        &target.join("connections.json"),
        &serde_json::to_vec_pretty(&ProjectConnectionStore::default()).unwrap(),
    )
    .unwrap();

    let error = workspace_connection_dir(&captured).unwrap_err();
    assert!(error.contains("both previous and current"));
    assert!(legacy.join("connections.json").is_file());
    assert!(target.join("connections.json").is_file());
}

#[cfg(unix)]
#[test]
fn connection_store_rejects_symlinks_and_non_owner_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("connections.json");
    let target = dir.path().join("target.json");
    fs::write(&target, b"{}").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, &store).unwrap();
    assert!(reject_unsafe_owner_file(&store).is_err());

    fs::remove_file(&store).unwrap();
    fs::write(&store, b"{}").unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(reject_unsafe_owner_file(&store).is_err());
    fs::set_permissions(&store, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(reject_unsafe_owner_file(&store).is_ok());
}

#[cfg(unix)]
#[test]
fn secure_replacement_never_follows_a_target_or_parent_symlink() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let target = dir.path().join("target.json");
    fs::write(&target, b"sentinel").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();

    let target_link = dir.path().join("connections.json");
    symlink(&target, &target_link).unwrap();
    assert!(safe_atomic_write_owner_file(&target_link, b"replacement").is_err());
    assert_eq!(fs::read(&target).unwrap(), b"sentinel");

    let real_parent = dir.path().join("real");
    fs::create_dir(&real_parent).unwrap();
    fs::set_permissions(&real_parent, fs::Permissions::from_mode(0o700)).unwrap();
    let parent_link = dir.path().join("linked");
    symlink(&real_parent, &parent_link).unwrap();
    assert!(safe_atomic_write_owner_file(&parent_link.join("connections.json"), b"data").is_err());
    assert!(!real_parent.join("connections.json").exists());
}
