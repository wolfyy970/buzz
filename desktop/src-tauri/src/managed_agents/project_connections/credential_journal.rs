use std::{collections::BTreeSet, fs, io};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[cfg(test)]
use super::atomic_write_json_restricted;
use super::{
    canonical_project_scope, capture_credential_target, delete_secrets_at_target, is_lower_hex,
    read_bounded_owner_file, reject_unsafe_owner_file, safe_atomic_write_owner_file,
    workspace_connection_dir, CapturedProjectConnectionScope, ProjectConnectionScope,
    ProjectConnectionStore,
};

const CREDENTIAL_JOURNAL_VERSION: u32 = 2;
const MAX_JOURNALED_GENERATIONS: usize = 2;
const MAX_CREDENTIAL_JOURNAL_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CredentialJournal {
    version: u32,
    project_scope: ProjectConnectionScope,
    connection_id: String,
    generations: Vec<String>,
}

fn journal_path(scope: &CapturedProjectConnectionScope) -> Result<std::path::PathBuf, String> {
    Ok(workspace_connection_dir(scope)?.join("credential-journal.json"))
}

fn activation_journal_path(
    workspace: &super::super::scope::WorkspaceAgentScope,
) -> std::path::PathBuf {
    workspace
        .definitions_dir
        .join("project-connections")
        .join("credential-journal.json")
}

fn validate_journal(journal: &CredentialJournal) -> Result<(), String> {
    let canonical_project = canonical_project_scope(&journal.project_scope)
        .map_err(|_| "Project connection credential recovery data is invalid.".to_string())?;
    if journal.version != CREDENTIAL_JOURNAL_VERSION
        || canonical_project != journal.project_scope
        || !is_lower_hex(&journal.connection_id, 32)
        || journal.generations.is_empty()
        || journal.generations.len() > MAX_JOURNALED_GENERATIONS
        || journal
            .generations
            .iter()
            .any(|generation| !is_lower_hex(generation, 32))
        || journal.generations.iter().collect::<BTreeSet<_>>().len() != journal.generations.len()
    {
        return Err("Project connection credential recovery data is invalid.".to_string());
    }
    Ok(())
}

fn read_journal(path: &std::path::Path) -> Result<Option<CredentialJournal>, String> {
    let Some(bytes) = read_bounded_owner_file(
        path,
        MAX_CREDENTIAL_JOURNAL_BYTES,
        "Project connection credential recovery data",
    )?
    else {
        return Ok(None);
    };
    let journal: CredentialJournal = serde_json::from_slice(&bytes)
        .map_err(|_| "Project connection credential recovery data is invalid.".to_string())?;
    validate_journal(&journal)?;
    Ok(Some(journal))
}

pub(super) fn validate_for_migration(path: &std::path::Path) -> Result<(), String> {
    read_journal(path).map(|_| ())
}

pub(super) fn project_for_scope_activation(
    workspace: &super::super::scope::WorkspaceAgentScope,
) -> Result<Option<ProjectConnectionScope>, String> {
    super::super::scope::validate_scope_generation(workspace)?;
    super::validate_ready_workspace_connection_directory(workspace)?;
    let path = activation_journal_path(workspace);
    let Some(parent) = path.parent() else {
        return Err("Project connection credential recovery data is invalid.".to_string());
    };
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("Buzz refused an unsafe Project connection directory.".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "failed to inspect Project connection directory {}: {error}",
                parent.display()
            ));
        }
    }
    let Some(journal) = read_journal(&path)? else {
        return Ok(None);
    };
    if journal.project_scope.relay_url != workspace.relay_url
        || !journal
            .project_scope
            .operator_pubkey
            .eq_ignore_ascii_case(&workspace.owner_pubkey)
        || super::workspace_scope_id(&journal.project_scope) != workspace.scope_id
    {
        return Err(
            "Project connection credential recovery data belongs to another workspace.".to_string(),
        );
    }
    Ok(Some(journal.project_scope))
}

fn generations_to_delete(
    journal: &CredentialJournal,
    store: &ProjectConnectionStore,
) -> Vec<String> {
    let referenced = store
        .connections
        .iter()
        .find(|connection| {
            connection.id == journal.connection_id
                && connection.project_scope == journal.project_scope
        })
        .filter(|connection| !connection.env_keys.is_empty())
        .map(|connection| connection.credential_generation.as_str());
    journal
        .generations
        .iter()
        .filter(|generation| Some(generation.as_str()) != referenced)
        .cloned()
        .collect()
}

pub(super) fn begin(
    app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
    connection_id: &str,
    generations: Vec<String>,
) -> Result<(), String> {
    let _ = app;
    super::validate_captured_scope_for_app(app, scope)?;
    let journal = CredentialJournal {
        version: CREDENTIAL_JOURNAL_VERSION,
        project_scope: scope.project.clone(),
        connection_id: connection_id.to_string(),
        generations,
    };
    validate_journal(&journal)?;
    let path = journal_path(scope)?;
    reject_unsafe_owner_file(&path)?;
    let bytes = serde_json::to_vec_pretty(&journal)
        .map_err(|error| format!("failed to prepare credential recovery data: {error}"))?;
    safe_atomic_write_owner_file(&path, &bytes)
}

pub(super) fn complete(
    app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
) -> Result<(), String> {
    let _ = app;
    super::validate_captured_scope_for_app(app, scope)?;
    let path = journal_path(scope)?;
    complete_path(&path)
}

fn complete_path(path: &std::path::Path) -> Result<(), String> {
    reject_unsafe_owner_file(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to clear Project connection credential recovery data: {error}"
        )),
    }
}

pub(super) fn reconcile(
    app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
    store: &ProjectConnectionStore,
) -> Result<(), String> {
    super::validate_captured_scope_for_app(app, scope)?;
    let path = journal_path(scope)?;
    reconcile_path(
        &path,
        store,
        Some(&scope.project),
        |connection_id, generation| {
            let target = capture_credential_target(app, scope, connection_id, generation)?;
            delete_secrets_at_target(&target)
        },
    )
}

fn reconcile_path(
    path: &std::path::Path,
    store: &ProjectConnectionStore,
    expected_project: Option<&ProjectConnectionScope>,
    mut delete_generation: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    let journal = match read_journal(path)? {
        Some(journal) => journal,
        None => return Ok(()),
    };
    if expected_project.is_some_and(|project| project != &journal.project_scope) {
        return Err(
            "Project connection credential recovery data belongs to another Project.".to_string(),
        );
    }
    for generation in generations_to_delete(&journal, store) {
        delete_generation(&journal.connection_id, &generation)?;
    }
    complete_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_agents::project_connections::{
        next_generation, ProjectConnectionHealth, ProjectConnectionScope, StoredProjectConnection,
        CONNECTION_STORE_VERSION,
    };

    fn project() -> ProjectConnectionScope {
        ProjectConnectionScope {
            relay_url: "ws://127.0.0.1:3000".to_string(),
            operator_pubkey: "a".repeat(64),
            project_address: format!("30621:{}:portable-agents", "a".repeat(64)),
        }
    }

    fn connection(generation: &str) -> StoredProjectConnection {
        StoredProjectConnection {
            id: "c".repeat(32),
            project_scope: project(),
            name: "Test".to_string(),
            provider: "Fixture".to_string(),
            capability_ids: Vec::new(),
            command: "/usr/bin/true".to_string(),
            args: Vec::new(),
            env_keys: vec!["TOKEN".to_string()],
            discovered_tools: Vec::new(),
            health: ProjectConnectionHealth::default(),
            executable_sha256: "d".repeat(64),
            generation: next_generation(),
            credential_generation: generation.to_string(),
            created_at: "2026-08-05T00:00:00Z".to_string(),
            updated_at: "2026-08-05T00:00:00Z".to_string(),
        }
    }

    fn store(connection: Option<StoredProjectConnection>) -> ProjectConnectionStore {
        ProjectConnectionStore {
            version: CONNECTION_STORE_VERSION,
            connections: connection.into_iter().collect(),
        }
    }

    #[test]
    fn restart_reconciliation_deletes_only_unreferenced_generations() {
        let old = "a".repeat(32);
        let new = "b".repeat(32);
        let cases = [
            (
                "create interrupted after credential write",
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![new.clone()],
                },
                store(None),
                vec![new.clone()],
            ),
            (
                "update interrupted before metadata swap",
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![old.clone(), new.clone()],
                },
                store(Some(connection(&old))),
                vec![new.clone()],
            ),
            (
                "update interrupted after metadata swap",
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![old.clone(), new.clone()],
                },
                store(Some(connection(&new))),
                vec![old.clone()],
            ),
            (
                "delete interrupted after metadata removal",
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![old.clone()],
                },
                store(None),
                vec![old.clone()],
            ),
        ];

        for (name, journal, store, expected) in cases {
            assert_eq!(generations_to_delete(&journal, &store), expected, "{name}");
        }
    }

    #[test]
    fn same_connection_id_in_another_project_does_not_retain_the_orphan() {
        let generation = "b".repeat(32);
        let journal = CredentialJournal {
            version: CREDENTIAL_JOURNAL_VERSION,
            project_scope: project(),
            connection_id: "c".repeat(32),
            generations: vec![generation.clone()],
        };
        let mut other_project_connection = connection(&generation);
        other_project_connection.project_scope.project_address =
            format!("30621:{}:another-project", "a".repeat(64));

        assert_eq!(
            generations_to_delete(&journal, &store(Some(other_project_connection))),
            [generation]
        );
    }

    #[test]
    fn activation_finds_the_exact_project_recorded_by_the_journal() {
        let _generation_guard = super::super::super::scope::SCOPE_GENERATION_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let base = tempfile::tempdir().unwrap();
        let generation = super::super::super::scope::next_scope_generation();
        let workspace = super::super::super::scope::WorkspaceAgentScope::new(
            "ws://127.0.0.1:3000".to_string(),
            "a".repeat(64),
            base.path(),
            generation,
        );
        super::super::super::scope_init::ensure_scope_ready(
            &workspace.scope_id,
            &workspace.definitions_dir,
            base.path(),
            &workspace.owner_pubkey,
        )
        .unwrap();
        let path = activation_journal_path(&workspace);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let journal = CredentialJournal {
            version: CREDENTIAL_JOURNAL_VERSION,
            project_scope: project(),
            connection_id: "c".repeat(32),
            generations: vec!["b".repeat(32)],
        };
        atomic_write_json_restricted(&path, &serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

        assert_eq!(
            project_for_scope_activation(&workspace).unwrap(),
            Some(project())
        );
    }

    #[test]
    fn reconciliation_retries_cleanup_before_removing_the_journal() {
        let old = "a".repeat(32);
        let new = "b".repeat(32);
        let cases = [
            (
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![new.clone()],
                },
                store(None),
                new.clone(),
            ),
            (
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![old.clone(), new.clone()],
                },
                store(Some(connection(&old))),
                new.clone(),
            ),
            (
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![old.clone(), new.clone()],
                },
                store(Some(connection(&new))),
                old.clone(),
            ),
            (
                CredentialJournal {
                    version: CREDENTIAL_JOURNAL_VERSION,
                    project_scope: project(),
                    connection_id: "c".repeat(32),
                    generations: vec![old.clone()],
                },
                store(None),
                old.clone(),
            ),
        ];

        for (journal, store, orphan) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("credential-journal.json");
            let bytes = serde_json::to_vec_pretty(&journal).unwrap();
            atomic_write_json_restricted(&path, &bytes).unwrap();

            let error = reconcile_path(&path, &store, Some(&journal.project_scope), |_, _| {
                Err("keyring unavailable".to_string())
            })
            .unwrap_err();
            assert_eq!(error, "keyring unavailable");
            assert!(path.exists(), "failed cleanup must preserve recovery data");

            let mut deleted = Vec::new();
            reconcile_path(
                &path,
                &store,
                Some(&journal.project_scope),
                |connection_id, generation| {
                    deleted.push((connection_id.to_string(), generation.to_string()));
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(deleted, [(journal.connection_id.clone(), orphan)]);
            assert!(!path.exists(), "successful retry must clear recovery data");
        }
    }
}
