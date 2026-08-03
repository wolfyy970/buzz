//! Project-owned tool connections for managed agents.
//!
//! Templates declare portable capabilities. This module keeps the concrete
//! MCP transport in a local, owner-only metadata store and its environment
//! values in the OS keyring. A managed-agent instance binds requirement ids to
//! connection ids. Nothing credential-bearing enters persona events, template
//! snapshots, or the managed-agent JSON store.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    sync::{Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _};
use uuid::Uuid;

use super::{
    atomic_write_json_restricted, managed_agents_base_dir, resolve_command, AgentProjectScope,
    AgentToolRequirement,
};
use crate::util::now_iso;

const CONNECTION_STORE_VERSION: u32 = 1;
const MAX_CONNECTIONS: usize = 128;
const MAX_NAME_BYTES: usize = 128;
const MAX_PROVIDER_BYTES: usize = 64;
const MAX_COMMAND_BYTES: usize = 1024;
const MAX_ARGS: usize = 128;
const MAX_ARG_BYTES: usize = 4096;
const MAX_ENV_KEYS: usize = 128;
const MAX_SECRET_BYTES: usize = 64 * 1024;
const HEALTH_STALE_AFTER_SECONDS: i64 = 24 * 60 * 60;

static PROJECT_CONNECTIONS_LOCK: Mutex<()> = Mutex::new(());

mod secrets;
use secrets::{delete_secrets, load_secrets, store_secrets};

/// Serialize connection and agent-binding mutations through one process-local
/// critical section. Agent create/update and connection delete use the same
/// guard so a connection cannot be assigned while its removal is being
/// approved.
pub(crate) fn lock_project_connections() -> MutexGuard<'static, ()> {
    PROJECT_CONNECTIONS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectConnectionHealthStatus {
    Ready,
    NotTested,
    CheckNeeded,
    SignInRequired,
    MissingAccess,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConnectionHealth {
    pub status: ProjectConnectionHealthStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Default for ProjectConnectionHealth {
    fn default() -> Self {
        Self {
            status: ProjectConnectionHealthStatus::NotTested,
            last_verified_at: None,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConnection {
    pub id: String,
    pub project_scope: AgentProjectScope,
    pub name: String,
    pub provider: String,
    /// Capabilities verified from the MCP server. Tool names are represented
    /// as stable `mcp.tool.<name>` identifiers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_ids: Vec<String>,
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Names only. Values live in the OS keyring.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discovered_tools: Vec<String>,
    #[serde(default)]
    pub health: ProjectConnectionHealth,
    /// Non-secret change token. Regenerated for transport edits, credential
    /// rotation, and health/capability verification.
    pub generation: u64,
    /// Opaque keyring generation used to replace credentials transactionally.
    pub credential_generation: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectConnectionRequest {
    pub project_scope: AgentProjectScope,
    pub name: String,
    pub provider: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Secret environment values. They are accepted on this write boundary
    /// and are never returned by any command.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectConnectionRequest {
    pub id: String,
    pub project_scope: AgentProjectScope,
    pub name: String,
    pub provider: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Changed or added secret values. Omitted keys retain their current value.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Explicit removals, separate from an empty value which may be a valid
    /// credential for some local servers.
    #[serde(default)]
    pub remove_env_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConnectionImpactAgent {
    pub pubkey: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConnectionImpact {
    pub connection_id: String,
    pub agents: Vec<ProjectConnectionImpactAgent>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolBindingIssue {
    pub requirement_id: String,
    pub label: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectConnectionStore {
    version: u32,
    connections: Vec<ProjectConnection>,
}

impl Default for ProjectConnectionStore {
    fn default() -> Self {
        Self {
            version: CONNECTION_STORE_VERSION,
            connections: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct MaterializedMcpServer {
    name: String,
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct MaterializedMcpDocument {
    version: u32,
    servers: Vec<MaterializedMcpServer>,
}

pub(crate) struct MaterializedProjectConnections {
    pub json: Vec<u8>,
    pub generation_hash: u64,
}

pub(crate) struct ProjectConnectionRollback {
    connection: ProjectConnection,
    secrets: BTreeMap<String, String>,
}

pub(crate) fn write_runtime_mcp_config(
    app: &AppHandle,
    runtime_id: &str,
    json: &[u8],
) -> Result<std::path::PathBuf, String> {
    if !runtime_id
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() || matches!(byte, b'_' | b'-'))
    {
        return Err("Buzz could not prepare a safe connection configuration path.".to_string());
    }
    let dir = managed_agents_base_dir(app)?.join("runtime-connections");
    match fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("Buzz refused an unsafe connection configuration directory.".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&dir).map_err(|error| {
                format!("failed to create connection configuration directory: {error}")
            })?;
        }
        Err(error) => {
            return Err(format!(
                "failed to inspect connection configuration directory: {error}"
            ));
        }
    }
    let path = dir.join(format!("{runtime_id}.json"));
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Buzz refused an unsafe connection configuration file.".to_string());
    }
    atomic_write_json_restricted(&path, json)?;
    Ok(path)
}

pub(crate) fn remove_runtime_mcp_config(path: &std::path::Path) {
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("buzz-desktop: failed to remove runtime MCP config: {error}");
        }
    }
}

fn connection_store_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(managed_agents_base_dir(app)?.join("project-connections.json"))
}

fn reject_unsafe_connection_store(path: &std::path::Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to inspect Project connection store {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Buzz refused an unsafe Project connection store path.".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "Project connection metadata is not owner-only. Fix its permissions before continuing."
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn load_store_unlocked(app: &AppHandle) -> Result<ProjectConnectionStore, String> {
    let path = connection_store_path(app)?;
    reject_unsafe_connection_store(&path)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProjectConnectionStore::default());
        }
        Err(error) => {
            return Err(format!(
                "failed to read Project connections from {}: {error}",
                path.display()
            ));
        }
    };
    let store: ProjectConnectionStore = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse Project connections: {error}"))?;
    if store.version != CONNECTION_STORE_VERSION {
        return Err(format!(
            "unsupported Project connection store version {}",
            store.version
        ));
    }
    if store.connections.len() > MAX_CONNECTIONS {
        return Err("Project connection store exceeds its connection limit".to_string());
    }
    Ok(store)
}

fn save_store_unlocked(app: &AppHandle, store: &ProjectConnectionStore) -> Result<(), String> {
    let path = connection_store_path(app)?;
    reject_unsafe_connection_store(&path)?;
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|error| format!("failed to serialize Project connections: {error}"))?;
    atomic_write_json_restricted(&path, &bytes)
}

fn next_generation() -> u64 {
    let value = Uuid::new_v4().as_u128();
    (value as u64) ^ ((value >> 64) as u64)
}

fn valid_stable_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(crate) fn canonical_project_scope(
    scope: &AgentProjectScope,
) -> Result<AgentProjectScope, String> {
    let canonical_relay = buzz_core_pkg::relay::normalize_relay_url(&scope.relay_url)
        .map_err(|_| "Choose a valid Buzz community before continuing.".to_string())?;
    if scope.operator_pubkey.len() != 64
        || !scope
            .operator_pubkey
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Buzz could not verify who owns these connections.".to_string());
    }
    let mut parts = scope.repo_address.splitn(3, ':');
    let kind = parts.next();
    let owner = parts.next();
    let d_tag = parts.next();
    if kind != Some("30617")
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
    let channel_id = Uuid::parse_str(&scope.channel_id)
        .map_err(|_| "This Project does not have a valid discussion channel.".to_string())?;
    Ok(AgentProjectScope {
        relay_url: canonical_relay,
        operator_pubkey: scope.operator_pubkey.to_ascii_lowercase(),
        repo_address: format!(
            "30617:{}:{}",
            owner.unwrap_or_default().to_ascii_lowercase(),
            d_tag.unwrap_or_default()
        ),
        channel_id: channel_id.to_string(),
    })
}

pub(crate) fn validate_project_scope(scope: &AgentProjectScope) -> Result<(), String> {
    if canonical_project_scope(scope)? != *scope {
        return Err("Buzz could not verify this Project's canonical identity.".to_string());
    }
    Ok(())
}

pub(crate) fn validate_project_scope_for_app(
    app: &AppHandle,
    scope: &AgentProjectScope,
) -> Result<(), String> {
    validate_project_scope(scope)?;
    let state = app.state::<crate::app_state::AppState>();
    let relay = buzz_core_pkg::relay::normalize_relay_url(
        &crate::relay::relay_ws_url_with_override(&state),
    )
    .map_err(|_| "Buzz could not verify the active community.".to_string())?;
    if scope.relay_url != relay {
        return Err("This Project belongs to another Buzz community.".to_string());
    }
    let operator = state
        .keys
        .lock()
        .map_err(|_| "Buzz could not verify the active identity.".to_string())?
        .public_key()
        .to_hex();
    if !scope.operator_pubkey.eq_ignore_ascii_case(&operator) {
        return Err("These connections belong to another Buzz identity.".to_string());
    }
    Ok(())
}

pub(crate) fn validate_tool_requirements(
    requirements: &[AgentToolRequirement],
) -> Result<(), String> {
    if requirements.len() > 32 {
        return Err("An agent template can require at most 32 tools.".to_string());
    }
    let mut ids = BTreeSet::new();
    for requirement in requirements {
        if !valid_stable_id(&requirement.id, 64) {
            return Err("Every tool requirement needs a stable id.".to_string());
        }
        if !ids.insert(requirement.id.to_ascii_lowercase()) {
            return Err(format!(
                "The tool requirement id '{}' is used more than once.",
                requirement.id
            ));
        }
        if requirement.label.trim().is_empty() || requirement.label.len() > 128 {
            return Err("Every tool requirement needs a short name.".to_string());
        }
        if !valid_capability_id(&requirement.capability) {
            return Err(format!(
                "The tool requirement '{}' has an invalid capability id.",
                requirement.label
            ));
        }
    }
    Ok(())
}

fn valid_capability_id(value: &str) -> bool {
    value.starts_with("mcp.tool.") && valid_stable_id(value, 192)
}

fn validate_connection_input(
    scope: &AgentProjectScope,
    name: &str,
    provider: &str,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    validate_project_scope(scope)?;
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err("Give this connection a short name.".to_string());
    }
    if provider.trim().is_empty() || provider.len() > MAX_PROVIDER_BYTES {
        return Err("Choose a provider for this connection.".to_string());
    }
    if command.trim().is_empty()
        || command.len() > MAX_COMMAND_BYTES
        || command.contains('\0')
        || command.contains('\n')
    {
        return Err("Enter a valid MCP server command.".to_string());
    }
    if args.len() > MAX_ARGS
        || args
            .iter()
            .any(|arg| arg.len() > MAX_ARG_BYTES || arg.contains('\0'))
    {
        return Err("The MCP server arguments exceed Buzz's safety limits.".to_string());
    }
    if env.len() > MAX_ENV_KEYS {
        return Err("This connection has too many secret values.".to_string());
    }
    let mut total = 0usize;
    for (key, value) in env {
        if !super::is_well_formed_env_key(key) || super::is_reserved_env_key(key) {
            return Err(format!(
                "'{key}' cannot be used as a connection secret name."
            ));
        }
        if value.contains('\0') {
            return Err(format!(
                "The value for '{key}' contains an invalid character."
            ));
        }
        total = total.saturating_add(key.len()).saturating_add(value.len());
    }
    if total > MAX_SECRET_BYTES {
        return Err("The connection secret values exceed Buzz's size limit.".to_string());
    }
    Ok(())
}

fn canonical_connection_command(command: &str) -> Result<String, String> {
    let trimmed = command.trim();
    let resolved =
        resolve_command(trimmed).ok_or_else(|| format!("Buzz could not find '{trimmed}'."))?;
    let canonical =
        fs::canonicalize(&resolved).map_err(|_| format!("Buzz could not verify '{trimmed}'."))?;
    let metadata = canonical
        .metadata()
        .map_err(|_| format!("Buzz could not verify '{trimmed}'."))?;
    if !canonical.is_absolute() || !metadata.is_file() {
        return Err(format!("'{trimmed}' is not an executable file."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("'{trimmed}' is not executable."));
        }
    }
    canonical
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| "The MCP server command path is not valid Unicode.".to_string())
}

fn health_for_display(mut connection: ProjectConnection) -> ProjectConnection {
    if connection.health.status == ProjectConnectionHealthStatus::Ready {
        let stale = connection
            .health
            .last_verified_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_none_or(|verified| {
                chrono::Utc::now().signed_duration_since(verified.with_timezone(&chrono::Utc))
                    > chrono::Duration::seconds(HEALTH_STALE_AFTER_SECONDS)
            });
        if stale {
            connection.health.status = ProjectConnectionHealthStatus::CheckNeeded;
        }
    }
    connection
}

pub fn list_project_connections(
    app: &AppHandle,
    project_scope: &AgentProjectScope,
) -> Result<Vec<ProjectConnection>, String> {
    let project_scope = canonical_project_scope(project_scope)?;
    validate_project_scope_for_app(app, &project_scope)?;
    let _guard = lock_project_connections();
    let mut connections: Vec<_> = load_store_unlocked(app)?
        .connections
        .into_iter()
        .filter(|connection| connection.project_scope == project_scope)
        .map(health_for_display)
        .collect();
    connections.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(connections)
}

pub fn create_project_connection(
    app: &AppHandle,
    mut input: CreateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    input.project_scope = canonical_project_scope(&input.project_scope)?;
    validate_project_scope_for_app(app, &input.project_scope)?;
    validate_connection_input(
        &input.project_scope,
        &input.name,
        &input.provider,
        &input.command,
        &input.args,
        &input.env,
    )?;
    let command = canonical_connection_command(&input.command)?;
    let _guard = lock_project_connections();
    let mut store = load_store_unlocked(app)?;
    if store.connections.len() >= MAX_CONNECTIONS {
        return Err("Buzz has reached the Project connection limit.".to_string());
    }
    let id = Uuid::new_v4().simple().to_string();
    let now = now_iso();
    let credential_generation = next_generation();
    let connection = ProjectConnection {
        id: id.clone(),
        project_scope: input.project_scope,
        name: input.name.trim().to_string(),
        provider: input.provider.trim().to_string(),
        capability_ids: Vec::new(),
        command,
        args: input.args,
        env_keys: input.env.keys().cloned().collect(),
        discovered_tools: Vec::new(),
        health: ProjectConnectionHealth::default(),
        generation: next_generation(),
        credential_generation,
        created_at: now.clone(),
        updated_at: now,
    };

    store_secrets(app, &id, credential_generation, &input.env)?;
    store.connections.push(connection.clone());
    if let Err(error) = save_store_unlocked(app, &store) {
        let _ = delete_secrets(app, &id, credential_generation);
        return Err(error);
    }
    Ok(connection)
}

pub fn update_project_connection(
    app: &AppHandle,
    mut input: UpdateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    input.project_scope = canonical_project_scope(&input.project_scope)?;
    validate_project_scope_for_app(app, &input.project_scope)?;
    validate_connection_input(
        &input.project_scope,
        &input.name,
        &input.provider,
        &input.command,
        &input.args,
        &input.env,
    )?;
    for key in &input.remove_env_keys {
        if !super::is_well_formed_env_key(key) {
            return Err("A secret name is invalid.".to_string());
        }
    }
    let command = canonical_connection_command(&input.command)?;
    let (previous_connection, previous_secrets) = {
        let _guard = lock_project_connections();
        let store = load_store_unlocked(app)?;
        let previous_connection = store
            .connections
            .iter()
            .find(|connection| connection.id == input.id)
            .cloned()
            .ok_or_else(|| "This connection no longer exists.".to_string())?;
        if previous_connection.project_scope != input.project_scope {
            return Err(
                "A connection cannot be moved to another Project. Add a new connection there."
                    .to_string(),
            );
        }
        let previous_secrets = load_secrets(app, &previous_connection)?;
        (previous_connection, previous_secrets)
    };
    let mut next_secrets = previous_secrets.clone();
    for key in input.remove_env_keys {
        next_secrets.remove(&key);
    }
    next_secrets.extend(input.env);
    validate_connection_input(
        &input.project_scope,
        &input.name,
        &input.provider,
        &input.command,
        &input.args,
        &next_secrets,
    )?;
    let credential_generation = next_generation();

    let mut updated = previous_connection.clone();
    updated.name = input.name.trim().to_string();
    updated.provider = input.provider.trim().to_string();
    updated.command = command;
    updated.args = input.args;
    updated.env_keys = next_secrets.keys().cloned().collect();
    updated.generation = next_generation();
    updated.credential_generation = credential_generation;
    updated.updated_at = now_iso();

    let tools = probe_mcp_connection(&updated, &next_secrets)?;
    updated.discovered_tools = tools.clone();
    updated.capability_ids = tools
        .iter()
        .map(|tool| format!("mcp.tool.{tool}"))
        .collect();
    updated.health = ProjectConnectionHealth {
        status: ProjectConnectionHealthStatus::Ready,
        last_verified_at: Some(now_iso()),
        detail: None,
    };
    store_secrets(app, &updated.id, credential_generation, &next_secrets)?;
    let _guard = lock_project_connections();
    let mut store = load_store_unlocked(app)?;
    let Some(index) = store
        .connections
        .iter()
        .position(|connection| connection.id == input.id)
    else {
        let _ = delete_secrets(app, &updated.id, credential_generation);
        return Err("This connection was removed while Buzz checked it.".to_string());
    };
    if store.connections[index].generation != previous_connection.generation {
        let _ = delete_secrets(app, &updated.id, credential_generation);
        return Err(
            "This connection changed while Buzz checked it. Review the latest version and try again."
                .to_string(),
        );
    }
    store.connections[index] = updated.clone();
    if let Err(error) = save_store_unlocked(app, &store) {
        let _ = delete_secrets(app, &updated.id, credential_generation);
        return Err(error);
    }
    Ok(updated)
}

pub(crate) fn snapshot_project_connection(
    app: &AppHandle,
    connection_id: &str,
) -> Result<ProjectConnectionRollback, String> {
    let _guard = lock_project_connections();
    let connection = load_store_unlocked(app)?
        .connections
        .into_iter()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| "This connection no longer exists.".to_string())?;
    let secrets = load_secrets(app, &connection)?;
    Ok(ProjectConnectionRollback {
        connection,
        secrets,
    })
}

pub(crate) fn restore_project_connection(
    app: &AppHandle,
    rollback: &ProjectConnectionRollback,
    expected_generation: u64,
) -> Result<(), String> {
    store_secrets(
        app,
        &rollback.connection.id,
        rollback.connection.credential_generation,
        &rollback.secrets,
    )?;
    let _guard = lock_project_connections();
    let mut store = load_store_unlocked(app)?;
    let connection = store
        .connections
        .iter_mut()
        .find(|connection| connection.id == rollback.connection.id)
        .ok_or_else(|| "This connection was removed before it could be restored.".to_string())?;
    if connection.generation != expected_generation {
        return Err(
            "This connection changed again before Buzz could restore it. The newer change was not overwritten."
                .to_string(),
        );
    }
    let failed_generation = connection.credential_generation;
    *connection = rollback.connection.clone();
    save_store_unlocked(app, &store)?;
    let _ = delete_secrets(app, &rollback.connection.id, failed_generation);
    Ok(())
}

pub(crate) fn finalize_project_connection_update(
    app: &AppHandle,
    rollback: &ProjectConnectionRollback,
) {
    let _ = delete_secrets(
        app,
        &rollback.connection.id,
        rollback.connection.credential_generation,
    );
}

pub fn project_connection_impact(
    app: &AppHandle,
    connection_id: &str,
) -> Result<ProjectConnectionImpact, String> {
    let records = super::load_managed_agents(app)
        .map_err(|_| "Buzz could not check which agents use this connection.".to_string())?;
    let mut agents: Vec<_> = records
        .into_iter()
        .filter(|record| {
            record
                .connection_bindings
                .values()
                .any(|id| id == connection_id)
        })
        .map(|record| ProjectConnectionImpactAgent {
            pubkey: record.pubkey,
            name: record.name,
        })
        .collect();
    agents.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.pubkey.cmp(&right.pubkey))
    });
    Ok(ProjectConnectionImpact {
        connection_id: connection_id.to_string(),
        agents,
    })
}

pub fn delete_project_connection(app: &AppHandle, connection_id: &str) -> Result<(), String> {
    let state = app.state::<crate::app_state::AppState>();
    let _agent_store_guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|_| "Buzz could not check which agents use this connection.".to_string())?;
    let _guard = lock_project_connections();
    let impact = project_connection_impact(app, connection_id)?;
    if !impact.agents.is_empty() {
        let names = impact
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "{names} use this connection. Change their connections before removing it."
        ));
    }
    let mut store = load_store_unlocked(app)?;
    let index = store
        .connections
        .iter()
        .position(|connection| connection.id == connection_id)
        .ok_or_else(|| "This connection no longer exists.".to_string())?;
    let removed = store.connections.remove(index);
    let secrets = load_secrets(app, &removed)?;
    save_store_unlocked(app, &store)?;
    let credential_generation = removed.credential_generation;
    if let Err(error) = delete_secrets(app, connection_id, credential_generation) {
        store.connections.push(removed);
        let _ = save_store_unlocked(app, &store);
        let _ = store_secrets(app, connection_id, credential_generation, &secrets);
        return Err(format!(
            "Buzz could not remove the saved credentials. The connection was not removed: {error}"
        ));
    }
    Ok(())
}

mod agent_runtime;
use agent_runtime::probe_mcp_connection;
pub use agent_runtime::test_project_connection;
#[cfg(test)]
use agent_runtime::validate_agent_bindings_against;
pub(crate) use agent_runtime::{
    agent_tool_binding_issues, current_connection_generation_hash,
    materialize_agent_project_connections, validate_agent_project_connections,
};

#[cfg(test)]
mod tests;
