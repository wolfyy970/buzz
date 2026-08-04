//! Project-owned MCP connection metadata and credentials.
//!
//! Connection metadata is partitioned by the active Buzz community and
//! identity. Secret values are never written to metadata or returned to the
//! webview after a write.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};
#[cfg(not(feature = "system-keyring"))]
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager as _};
use uuid::Uuid;

#[cfg(test)]
use super::atomic_write_json_restricted;
use crate::util::now_iso;
#[cfg(feature = "system-keyring")]
use crate::{app_state::keyring_service, secret_store::SecretStore};

const CONNECTION_STORE_VERSION: u32 = 1;
/// Maximum number of connection records in one workspace store.
const MAX_WORKSPACE_CONNECTIONS: usize = 128;
/// Maximum number of connection records attached to one Project.
const MAX_PROJECT_CONNECTIONS: usize = 32;
const MAX_CONNECTION_STORE_BYTES: u64 = 1024 * 1024;
/// Older builds accepted the same 128-record envelope without a serialized
/// store cap. Keep a bounded compatibility reader so those stores can be
/// listed, edited down, and migrated instead of becoming unreadable.
const MAX_LEGACY_CONNECTION_STORE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_NAME_BYTES: usize = 128;
const MAX_PROVIDER_BYTES: usize = 64;
const MAX_COMMAND_BYTES: usize = 1024;
const MAX_ARGS: usize = 128;
const MAX_ARG_BYTES: usize = 4096;
const MAX_ENV_KEYS: usize = 128;
const MAX_SECRET_BYTES: usize = 64 * 1024;
/// JSON may encode one input byte as a six-byte `\u00xx` escape. Keys,
/// separators, braces, and quotes add at most six punctuation bytes per entry.
const MAX_SECRET_FILE_BYTES: u64 = (MAX_SECRET_BYTES * 6 + MAX_ENV_KEYS * 6 + 2) as u64;
const HEALTH_STALE_AFTER_SECONDS: i64 = 24 * 60 * 60;

static PROJECT_CONNECTIONS_LOCK: Mutex<()> = Mutex::new(());

mod approval;
mod connection_data;
mod credential_journal;
mod credential_target;
mod operation_lease;
mod operations;
mod transactions;
#[cfg(test)]
use approval::executable_sha256;
use approval::{approved_execution_sha256, canonical_connection_command};
pub use connection_data::*;
use credential_target::*;
pub(crate) use operation_lease::ScopeOperationCoordinator;
pub(crate) use operations::{
    create_project_connection_at, delete_project_connection_at, list_project_connections_at,
    update_project_connection_at,
};
use transactions::{commit_delete, commit_update, UpdateTransaction};
mod store;
#[cfg(any(test, not(feature = "system-keyring")))]
use store::read_bounded_file;
#[cfg(test)]
use store::validate_stored_connection;
use store::{load_store_unlocked, save_store_unlocked};

pub(super) fn lock_project_connections() -> MutexGuard<'static, ()> {
    PROJECT_CONNECTIONS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConnectionScope {
    pub relay_url: String,
    pub operator_pubkey: String,
    /// Canonical NIP-MP Project coordinate (`30621:<owner>:<d-tag>`).
    ///
    /// Legacy one-repository Projects use their NIP-34 repository coordinate
    /// (`30617:<owner>:<d-tag>`).
    #[serde(alias = "repoAddress")]
    pub project_address: String,
}

pub(crate) struct CapturedProjectConnectionScope {
    pub project: ProjectConnectionScope,
    pub workspace: super::scope::WorkspaceAgentScope,
    operation: operation_lease::ScopeOperationLease,
}

fn next_generation() -> String {
    Uuid::new_v4().simple().to_string()
}

pub(super) fn connection_mcp_server_name(connection_id: &str) -> String {
    format!("project_{connection_id}")
}

fn valid_stable_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(super) fn canonical_project_scope(
    scope: &ProjectConnectionScope,
) -> Result<ProjectConnectionScope, String> {
    let relay_url = buzz_core_pkg::relay::normalize_relay_url(&scope.relay_url)
        .map_err(|_| "Choose a valid Buzz community before continuing.".to_string())?;
    if scope.operator_pubkey.len() != 64
        || !scope
            .operator_pubkey
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Buzz could not verify who owns these connections.".to_string());
    }
    let mut parts = scope.project_address.splitn(3, ':');
    let kind = parts.next();
    let owner = parts.next();
    let d_tag = parts.next();
    if !matches!(kind, Some("30617") | Some("30621"))
        || !owner.is_some_and(|value| {
            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || !d_tag.is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 256
                && !value.chars().any(char::is_control)
                && !value.contains(':')
        })
    {
        return Err("Choose a valid Buzz Project before continuing.".to_string());
    }
    Ok(ProjectConnectionScope {
        relay_url,
        operator_pubkey: scope.operator_pubkey.to_ascii_lowercase(),
        project_address: format!(
            "{}:{}:{}",
            kind.unwrap_or_default(),
            owner.unwrap_or_default().to_ascii_lowercase(),
            d_tag.unwrap_or_default()
        ),
    })
}

pub(crate) fn capture_project_scope_for_app(
    app: &AppHandle,
    scope: &ProjectConnectionScope,
) -> Result<CapturedProjectConnectionScope, String> {
    let canonical = canonical_project_scope(scope)?;
    let state = app.state::<crate::app_state::AppState>();
    let workspace = state
        .capture_active_scope()
        .ok_or_else(|| "Choose a Buzz community before managing connections.".to_string())?;
    super::scope::validate_scope_generation(&workspace)?;
    if canonical.relay_url != workspace.relay_url {
        return Err("This Project belongs to another Buzz community.".to_string());
    }
    if !canonical
        .operator_pubkey
        .eq_ignore_ascii_case(&workspace.owner_pubkey)
    {
        return Err("These connections belong to another Buzz identity.".to_string());
    }
    if workspace.scope_id != workspace_scope_id(&canonical) {
        return Err("This Project belongs to another Buzz workspace.".to_string());
    }
    let operation = state
        .project_connection_operations
        .acquire(workspace.generation)?;
    Ok(CapturedProjectConnectionScope {
        project: canonical,
        workspace,
        operation,
    })
}

fn capture_reconciliation_scope_for_app(
    app: &AppHandle,
) -> Result<Option<CapturedProjectConnectionScope>, String> {
    let state = app.state::<crate::app_state::AppState>();
    let workspace = state
        .capture_active_scope()
        .ok_or_else(|| "Choose a Buzz community before managing connections.".to_string())?;
    super::scope::validate_scope_generation(&workspace)?;
    let Some(project) = credential_journal::project_for_scope_activation(&workspace)? else {
        return Ok(None);
    };
    let operation = state
        .project_connection_operations
        .acquire(workspace.generation)?;
    Ok(Some(CapturedProjectConnectionScope {
        project,
        workspace,
        operation,
    }))
}

/// Start credential-journal recovery for the newly active workspace.
///
/// The caller invokes this while `workspace_transition` still serializes scope
/// changes. Capturing the generation lease here makes the background recovery
/// linearizable with the next workspace switch.
pub(crate) fn reconcile_project_connections_on_scope_activation(app: &AppHandle) {
    let captured = match capture_reconciliation_scope_for_app(app) {
        Ok(Some(captured)) => captured,
        Ok(None) => return,
        Err(error) => {
            eprintln!(
                "buzz-desktop: Project connection activation recovery could not start: {error}"
            );
            return;
        }
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_project_connections();
        if let Err(error) = load_store_unlocked(&app, &captured) {
            eprintln!("buzz-desktop: Project connection activation recovery failed: {error}");
        }
    });
}

fn validate_captured_scope_for_app(
    app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
) -> Result<(), String> {
    scope.operation.check_active()?;
    super::scope::validate_scope_generation(&scope.workspace)?;
    let active = app
        .state::<crate::app_state::AppState>()
        .capture_active_scope()
        .ok_or_else(|| "Choose a Buzz community before managing connections.".to_string())?;
    if active.generation != scope.workspace.generation
        || active.scope_id != scope.workspace.scope_id
        || active.relay_url != scope.workspace.relay_url
        || !active
            .owner_pubkey
            .eq_ignore_ascii_case(&scope.workspace.owner_pubkey)
    {
        return Err(
            "This Project connection operation became stale after the workspace changed."
                .to_string(),
        );
    }
    Ok(())
}

fn with_scope_commit_fence<T>(
    scope: &CapturedProjectConnectionScope,
    commit: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    scope.operation.check_active()?;
    if super::scope::current_scope_generation() != scope.workspace.generation {
        return Err(
            "This Project connection operation became stale after the workspace changed."
                .to_string(),
        );
    }
    commit()
}

fn workspace_scope_id(scope: &ProjectConnectionScope) -> String {
    super::scope::derive_scope_id(&scope.relay_url, &scope.operator_pubkey)
}

#[cfg(unix)]
fn ensure_owner_only_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("Buzz refused an unsafe Project connection directory.".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| {
                format!(
                    "failed to create Project connection directory {}: {error}",
                    path.display()
                )
            })?;
        }
        Err(error) => {
            return Err(format!(
                "failed to inspect Project connection directory {}: {error}",
                path.display()
            ));
        }
    }
    use rustix::fs::{Mode, OFlags};
    let directory = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        format!(
            "failed to securely open Project connection directory {}: {error}",
            path.display()
        )
    })?;
    rustix::fs::fchmod(&directory, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(|error| {
        format!(
            "failed to protect Project connection directory {}: {error}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn ensure_owner_only_directory(_path: &Path) -> Result<(), String> {
    Err(
        "Buzz cannot safely store Project connections on this platform because reparse-point-safe directory access is unavailable."
            .to_string(),
    )
}

fn validate_ready_workspace_connection_directory(
    workspace: &super::scope::WorkspaceAgentScope,
) -> Result<(), String> {
    let path = &workspace.definitions_dir;
    let metadata = fs::symlink_metadata(path).map_err(|_| {
        "Buzz could not open the initialized workspace connection store.".to_string()
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Buzz refused an unsafe workspace connection store.".to_string());
    }
    if !super::scope_init::scope_is_ready(path) {
        return Err("Finish opening this Buzz workspace before managing connections.".to_string());
    }
    let manifest_path = path.join("_manifest.json");
    let manifest_bytes =
        read_bounded_regular_file_no_follow(&manifest_path, 64 * 1024, "workspace manifest")
            .map_err(|_| "Buzz could not verify the workspace connection store.".to_string())?
            .ok_or_else(|| "Buzz could not verify the workspace connection store.".to_string())?;
    let manifest: super::scope_init::ScopeManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "Buzz could not verify the workspace connection store.".to_string())?;
    if manifest.scope_id != workspace.scope_id {
        return Err(
            "Buzz refused a workspace connection store owned by another scope.".to_string(),
        );
    }
    Ok(())
}

fn validate_ready_scope_directory(scope: &CapturedProjectConnectionScope) -> Result<(), String> {
    validate_ready_workspace_connection_directory(&scope.workspace)
}

fn validate_legacy_connection_directory(
    path: &Path,
    scope: &CapturedProjectConnectionScope,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "Buzz could not inspect the previous connection store.".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Buzz refused an unsafe previous connection store.".to_string());
    }
    for entry in fs::read_dir(path)
        .map_err(|_| "Buzz could not inspect the previous connection store.".to_string())?
    {
        let entry = entry
            .map_err(|_| "Buzz could not inspect the previous connection store.".to_string())?;
        let name = entry.file_name();
        if name != "connections.json" && name != "credential-journal.json" && name != "secrets" {
            return Err("The previous connection store contains an unknown file.".to_string());
        }
        let entry_metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| "Buzz could not inspect the previous connection store.".to_string())?;
        if entry_metadata.file_type().is_symlink() {
            return Err("Buzz refused an unsafe previous connection store.".to_string());
        }
    }

    let store_path = path.join("connections.json");
    let store = match read_bounded_owner_file(
        &store_path,
        MAX_LEGACY_CONNECTION_STORE_BYTES,
        "Project connection store",
    )? {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "The previous Project connection store is invalid.".to_string())?,
        None => ProjectConnectionStore::default(),
    };
    validate_store_for_scope(&store, scope)?;
    credential_journal::validate_for_migration(&path.join("credential-journal.json"))?;

    let secrets = path.join("secrets");
    match fs::symlink_metadata(&secrets) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("Buzz refused an unsafe previous credential store.".to_string());
        }
        Ok(_) => {
            for entry in fs::read_dir(&secrets)
                .map_err(|_| "Buzz could not inspect the previous credential store.".to_string())?
            {
                let entry = entry.map_err(|_| {
                    "Buzz could not inspect the previous credential store.".to_string()
                })?;
                read_bounded_owner_file(
                    &entry.path(),
                    MAX_SECRET_FILE_BYTES,
                    "Project connection credentials",
                )?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err("Buzz could not inspect the previous credential store.".to_string());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn rename_directory_no_follow(source: &Path, target: &Path) -> Result<(), String> {
    use rustix::fs::{Mode, OFlags};

    let source_parent = source
        .parent()
        .ok_or_else(|| "Buzz could not resolve the previous connection store.".to_string())?;
    let target_parent = target
        .parent()
        .ok_or_else(|| "Buzz could not resolve the current connection store.".to_string())?;
    let source_name = source
        .file_name()
        .ok_or_else(|| "Buzz could not resolve the previous connection store.".to_string())?;
    let target_name = target
        .file_name()
        .ok_or_else(|| "Buzz could not resolve the current connection store.".to_string())?;
    let source_parent_fd = rustix::fs::open(
        source_parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("Buzz could not securely open the previous store: {error}"))?;
    let target_parent_fd = rustix::fs::open(
        target_parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("Buzz could not securely open the current store: {error}"))?;
    rustix::fs::renameat(
        &source_parent_fd,
        source_name,
        &target_parent_fd,
        target_name,
    )
    .map_err(|error| format!("Buzz could not migrate the previous connection store: {error}"))?;
    rustix::fs::fsync(&source_parent_fd).map_err(|error| {
        format!(
            "failed to sync previous connection store directory {}: {error}",
            source_parent.display()
        )
    })?;
    rustix::fs::fsync(&target_parent_fd).map_err(|error| {
        format!(
            "failed to sync current connection store directory {}: {error}",
            target_parent.display()
        )
    })
}

#[cfg(not(unix))]
fn rename_directory_no_follow(_source: &Path, _target: &Path) -> Result<(), String> {
    Err(
        "Buzz cannot safely migrate Project connections on this platform because secure no-follow directory replacement is unavailable."
            .to_string(),
    )
}

#[cfg(unix)]
fn remove_empty_directory_no_follow(path: &Path) -> Result<(), String> {
    use rustix::fs::{AtFlags, Mode, OFlags};

    let parent = path
        .parent()
        .ok_or_else(|| "Buzz could not resolve the connection store migration.".to_string())?;
    let name = path
        .file_name()
        .ok_or_else(|| "Buzz could not resolve the connection store migration.".to_string())?;
    let parent_fd = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("Buzz could not securely open the connection store: {error}"))?;
    rustix::fs::unlinkat(&parent_fd, name, AtFlags::REMOVEDIR)
        .map_err(|_| "Buzz could not prepare the connection store migration.".to_string())
}

#[cfg(not(unix))]
fn remove_empty_directory_no_follow(_path: &Path) -> Result<(), String> {
    Err(
        "Buzz cannot safely migrate Project connections on this platform because secure no-follow directory removal is unavailable."
            .to_string(),
    )
}

fn migrate_legacy_connection_directory(
    scope: &CapturedProjectConnectionScope,
    target: &Path,
) -> Result<(), String> {
    let scopes_dir = scope
        .workspace
        .definitions_dir
        .parent()
        .ok_or_else(|| "Buzz could not resolve the workspace store.".to_string())?;
    let base_dir = scopes_dir
        .parent()
        .ok_or_else(|| "Buzz could not resolve the workspace store.".to_string())?;
    let legacy = base_dir
        .join("project-connections")
        .join(&scope.workspace.scope_id);
    if !legacy.exists() {
        return Ok(());
    }
    validate_legacy_connection_directory(&legacy, scope)?;

    if target.exists() {
        let mut entries = fs::read_dir(target)
            .map_err(|_| "Buzz could not inspect the new connection store.".to_string())?;
        if entries
            .next()
            .transpose()
            .map_err(|_| "Buzz could not inspect the new connection store.".to_string())?
            .is_some()
        {
            return Err(
                "Buzz found both previous and current Project connection stores. Nothing was changed."
                    .to_string(),
            );
        }
        remove_empty_directory_no_follow(target)?;
    }

    rename_directory_no_follow(&legacy, target)?;
    if let Err(error) = validate_legacy_connection_directory(target, scope) {
        let _ = rename_directory_no_follow(target, &legacy);
        return Err(error);
    }
    Ok(())
}

fn workspace_connection_dir(scope: &CapturedProjectConnectionScope) -> Result<PathBuf, String> {
    super::scope::validate_scope_generation(&scope.workspace)?;
    validate_ready_scope_directory(scope)?;
    let connections_dir = scope.workspace.definitions_dir.join("project-connections");
    migrate_legacy_connection_directory(scope, &connections_dir)?;
    ensure_owner_only_directory(&connections_dir)?;
    Ok(connections_dir)
}

fn connection_store_path(scope: &CapturedProjectConnectionScope) -> Result<PathBuf, String> {
    Ok(workspace_connection_dir(scope)?.join("connections.json"))
}

fn reject_unsafe_owner_file(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to inspect Project connection file {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Buzz refused an unsafe Project connection file.".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "Project connection data is not owner-only. Fix its permissions before continuing."
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// Write through an already-open parent directory so a target symlink or
/// parent-path swap cannot redirect secret bytes between validation and
/// replacement. On platforms without a directory-handle-relative no-follow
/// replacement primitive, Project connection writes fail closed.
#[cfg(unix)]
pub(super) fn safe_atomic_write_owner_file(path: &Path, payload: &[u8]) -> Result<(), String> {
    use rustix::fs::{AtFlags, Mode, OFlags};

    let parent = path
        .parent()
        .ok_or_else(|| "Buzz could not resolve the Project connection directory.".to_string())?;
    let name = path
        .file_name()
        .ok_or_else(|| "Buzz could not resolve the Project connection file.".to_string())?;
    let parent_fd = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("Buzz could not securely open {}: {error}", parent.display()))?;
    let parent_file = fs::File::from(parent_fd);
    let parent_metadata = parent_file
        .metadata()
        .map_err(|error| format!("failed to inspect {}: {error}", parent.display()))?;
    if !parent_metadata.is_dir() {
        return Err("Buzz refused an unsafe Project connection directory.".to_string());
    }
    use std::os::unix::fs::PermissionsExt as _;
    if parent_metadata.permissions().mode() & 0o077 != 0 {
        return Err(
            "Project connection data directory is not owner-only. Fix its permissions before continuing."
                .to_string(),
        );
    }
    reject_unsafe_owner_file(path)?;

    let temporary_name = format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        Uuid::new_v4().simple()
    );
    let temporary_fd = rustix::fs::openat(
        &parent_file,
        temporary_name.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| {
        format!("Buzz could not create a secure temporary Project connection file: {error}")
    })?;
    let mut temporary = fs::File::from(temporary_fd);
    let write_result = temporary
        .write_all(payload)
        .and_then(|()| temporary.sync_all());
    if let Err(error) = write_result {
        let _ = rustix::fs::unlinkat(&parent_file, temporary_name.as_str(), AtFlags::empty());
        return Err(format!(
            "failed to write Project connection data securely: {error}"
        ));
    }
    drop(temporary);
    if let Err(error) =
        rustix::fs::renameat(&parent_file, temporary_name.as_str(), &parent_file, name)
    {
        let _ = rustix::fs::unlinkat(&parent_file, temporary_name.as_str(), AtFlags::empty());
        return Err(format!(
            "failed to replace Project connection data securely: {error}"
        ));
    }
    rustix::fs::fsync(&parent_file)
        .map_err(|error| format!("failed to sync {}: {error}", parent.display()))
}

#[cfg(not(unix))]
pub(super) fn safe_atomic_write_owner_file(_path: &Path, _payload: &[u8]) -> Result<(), String> {
    Err(
        "Buzz cannot safely store Project connections on this platform because secure no-follow replacement is unavailable."
            .to_string(),
    )
}

pub(super) fn read_bounded_owner_file(
    path: &Path,
    max_bytes: u64,
    label: &str,
) -> Result<Option<Vec<u8>>, String> {
    let file = match open_owner_file_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "failed to read {label} from {}: {error}",
                path.display()
            ));
        }
    };
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {label} from {}: {error}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("{label} exceeds its size limit."));
    }
    Ok(Some(bytes))
}

fn read_bounded_regular_file_no_follow(
    path: &Path,
    max_bytes: u64,
    label: &str,
) -> Result<Option<Vec<u8>>, String> {
    let file = match open_regular_file_no_follow(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "failed to read {label} from {}: {error}",
                path.display()
            ));
        }
    };
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {label} from {}: {error}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("{label} exceeds its size limit."));
    }
    Ok(Some(bytes))
}

fn open_owner_file_no_follow(path: &Path) -> std::io::Result<fs::File> {
    open_regular_file_no_follow(path, true)
}

fn open_regular_file_no_follow(path: &Path, require_owner_only: bool) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if require_owner_only && metadata.permissions().mode() & 0o077 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "file is not owner-only",
            ));
        }
    }
    Ok(file)
}

fn load_store_unlocked(
    app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
) -> Result<ProjectConnectionStore, String> {
    let path = connection_store_path(scope)?;
    let store = match read_bounded_owner_file(
        &path,
        MAX_LEGACY_CONNECTION_STORE_BYTES,
        "Project connection store",
    )? {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to parse Project connections: {error}"))?,
        None => ProjectConnectionStore::default(),
    };
    validate_store_for_scope(&store, scope)?;
    credential_journal::reconcile(app, scope, &store)?;
    Ok(store)
}

fn validate_store_for_scope(
    store: &ProjectConnectionStore,
    scope: &CapturedProjectConnectionScope,
) -> Result<(), String> {
    if store.version != CONNECTION_STORE_VERSION {
        return Err(format!(
            "unsupported Project connection store version {}",
            store.version
        ));
    }
    if store.connections.len() > MAX_WORKSPACE_CONNECTIONS {
        return Err("Project connection store exceeds its workspace limit.".to_string());
    }
    let mut connection_ids = BTreeSet::new();
    for connection in &store.connections {
        validate_stored_connection(connection)?;
        if !connection_ids.insert(connection.id.as_str()) {
            return Err("Project connection metadata contains duplicate identities.".to_string());
        }
        if connection.project_scope.relay_url != scope.workspace.relay_url
            || !connection
                .project_scope
                .operator_pubkey
                .eq_ignore_ascii_case(&scope.workspace.owner_pubkey)
        {
            return Err("Project connection metadata belongs to another workspace.".to_string());
        }
    }
    Ok(())
}

fn save_store_unlocked(
    app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
    store: &ProjectConnectionStore,
) -> Result<(), String> {
    validate_store_for_scope(store, scope)?;
    validate_captured_scope_for_app(app, scope)?;
    let path = connection_store_path(scope)?;
    reject_unsafe_owner_file(&path)?;
    let bytes = serialize_store_bounded(store)?;
    // The generation lease is the commit fence. A workspace transition
    // revokes the generation and drains every lease before publishing its new
    // scope. Do not take `active_agent_scope` here: the transition holds that
    // lock while draining, so a leased save waiting for the same lock would
    // deadlock the switch.
    //
    // Preparation, credential I/O, and MCP execution happen before this short
    // synchronous boundary. The lease stays alive through the atomic replace.
    with_scope_commit_fence(scope, || safe_atomic_write_owner_file(&path, &bytes))
}

fn serialize_store_bounded(store: &ProjectConnectionStore) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|error| format!("failed to serialize Project connections: {error}"))?;
    if bytes.len() as u64 > MAX_LEGACY_CONNECTION_STORE_BYTES {
        return Err("Project connection store exceeds its compatibility size limit.".to_string());
    }
    Ok(bytes)
}

fn validate_new_store_size(store: &ProjectConnectionStore) -> Result<(), String> {
    if serialize_store_bounded(store)?.len() as u64 > MAX_CONNECTION_STORE_BYTES {
        return Err("Project connection store exceeds its size limit.".to_string());
    }
    Ok(())
}

fn validate_updated_store_size(
    previous: &ProjectConnectionStore,
    candidate: &ProjectConnectionStore,
) -> Result<(), String> {
    let previous_size = serialize_store_bounded(previous)?.len() as u64;
    let candidate_size = serialize_store_bounded(candidate)?.len() as u64;
    if candidate_size > MAX_CONNECTION_STORE_BYTES && candidate_size > previous_size {
        return Err(
            "This change would make the legacy Project connection store larger. Remove or shorten connection data first."
                .to_string(),
        );
    }
    Ok(())
}

mod probe;
pub(crate) use probe::test_project_connection_at;

#[cfg(test)]
mod tests;
