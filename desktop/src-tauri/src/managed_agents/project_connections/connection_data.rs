use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectConnectionHealthStatus {
    Ready,
    NotTested,
    CheckNeeded,
    ApprovalRequired,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConnection {
    pub id: String,
    pub project_scope: ProjectConnectionScope,
    pub name: String,
    pub provider: String,
    pub capability_ids: Vec<String>,
    pub command: String,
    pub args: Vec<String>,
    /// Names only. Values are never returned by a Tauri command.
    pub env_keys: Vec<String>,
    pub discovered_tools: Vec<String>,
    pub health: ProjectConnectionHealth,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredProjectConnection {
    pub(super) id: String,
    pub(super) project_scope: ProjectConnectionScope,
    pub(super) name: String,
    pub(super) provider: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) capability_ids: Vec<String>,
    pub(super) command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) env_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) discovered_tools: Vec<String>,
    #[serde(default)]
    pub(super) health: ProjectConnectionHealth,
    pub(super) executable_sha256: String,
    pub(super) generation: String,
    pub(super) credential_generation: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

impl From<StoredProjectConnection> for ProjectConnection {
    fn from(connection: StoredProjectConnection) -> Self {
        Self {
            id: connection.id,
            project_scope: connection.project_scope,
            name: connection.name,
            provider: connection.provider,
            capability_ids: connection.capability_ids,
            command: connection.command,
            args: connection.args,
            env_keys: connection.env_keys,
            discovered_tools: connection.discovered_tools,
            health: connection.health,
            created_at: connection.created_at,
            updated_at: connection.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectConnectionRequest {
    pub project_scope: ProjectConnectionScope,
    pub name: String,
    pub provider: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Secret environment values. They are write-only at this boundary.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub execution_acknowledged: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectConnectionRequest {
    pub id: String,
    pub project_scope: ProjectConnectionScope,
    pub name: String,
    pub provider: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Changed or added values. Omitted keys retain their saved value.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub remove_env_keys: Vec<String>,
    #[serde(default)]
    pub execution_acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectConnectionStore {
    pub(super) version: u32,
    pub(super) connections: Vec<StoredProjectConnection>,
}

impl Default for ProjectConnectionStore {
    fn default() -> Self {
        Self {
            version: CONNECTION_STORE_VERSION,
            connections: Vec::new(),
        }
    }
}

pub(super) fn serialize_secrets(env: &BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    let serialized = serde_json::to_vec(env)
        .map_err(|error| format!("failed to prepare connection credentials: {error}"))?;
    if serialized.len() as u64 > MAX_SECRET_FILE_BYTES {
        return Err(
            "The connection secret values exceed Buzz's serialized size limit.".to_string(),
        );
    }
    Ok(serialized)
}

#[cfg(any(test, feature = "system-keyring"))]
pub(super) fn store_verified_secret<Write, Verify, Delete>(
    mut write: Write,
    mut verify: Verify,
    mut delete: Delete,
) -> Result<(), String>
where
    Write: FnMut() -> Result<(), String>,
    Verify: FnMut() -> Result<bool, String>,
    Delete: FnMut() -> Result<(), String>,
{
    let failure = match write() {
        Ok(()) => match verify() {
            Ok(true) => return Ok(()),
            Ok(false) | Err(_) => "Buzz could not verify the saved credentials.",
        },
        Err(_) => "Buzz could not save these credentials in the system keyring.",
    };

    if delete().is_err() {
        return Err(format!(
            "{failure} Buzz also could not remove the unverified credentials."
        ));
    }
    Err(failure.to_string())
}

pub(super) fn store_secrets_at_target(
    target: &CapturedCredentialTarget,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let serialized = serialize_secrets(env)?;
    #[cfg(feature = "system-keyring")]
    {
        let raw = String::from_utf8(serialized)
            .map_err(|_| "Buzz could not prepare these credentials.".to_string())?;
        let store = SecretStore::shared(keyring_service());
        store_verified_secret(
            || store.store(&target.key, &raw),
            || store.verify_stored_raw(&target.key, &raw),
            || store.delete(&target.key),
        )?;
    }
    #[cfg(not(feature = "system-keyring"))]
    {
        reject_unsafe_owner_file(&target.path)?;
        safe_atomic_write_owner_file(&target.path, &serialized)?;
    }
    Ok(())
}

pub(super) fn load_secrets(
    _app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
    connection: &StoredProjectConnection,
) -> Result<BTreeMap<String, String>, String> {
    if connection.env_keys.is_empty() {
        return Ok(BTreeMap::new());
    }
    let target = capture_credential_target(
        _app,
        scope,
        &connection.id,
        &connection.credential_generation,
    )?;
    #[cfg(feature = "system-keyring")]
    let raw = SecretStore::shared(keyring_service())
        .load(&target.key)
        .map_err(|_| {
            format!(
                "Sign in again to '{}'. Buzz could not read its saved credentials.",
                connection.name
            )
        })?
        .ok_or_else(|| {
            format!(
                "Sign in again to '{}'. Its saved credentials are missing.",
                connection.name
            )
        })?
        .into_bytes();
    #[cfg(not(feature = "system-keyring"))]
    let raw = {
        read_bounded_owner_file(
            &target.path,
            MAX_SECRET_FILE_BYTES,
            "Project connection credentials",
        )
        .map_err(|_| {
            format!(
                "Sign in again to '{}'. Its saved credentials are invalid.",
                connection.name
            )
        })?
        .ok_or_else(|| {
            format!(
                "Sign in again to '{}'. Its saved credentials are missing.",
                connection.name
            )
        })?
    };
    let env: BTreeMap<String, String> = serde_json::from_slice(&raw).map_err(|_| {
        format!(
            "Sign in again to '{}'. Its credentials are invalid.",
            connection.name
        )
    })?;
    let actual: BTreeSet<&str> = env.keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = connection.env_keys.iter().map(String::as_str).collect();
    if actual != expected {
        return Err(format!(
            "Sign in again to '{}'. Its saved credentials are incomplete.",
            connection.name
        ));
    }
    Ok(env)
}

pub(super) fn delete_secrets(
    _app: &AppHandle,
    scope: &CapturedProjectConnectionScope,
    id: &str,
    credential_generation: &str,
) -> Result<(), String> {
    let target = capture_credential_target(_app, scope, id, credential_generation)?;
    delete_secrets_at_target(&target)
}

/// Remove one exact credential generation already authorized by the operation.
///
/// This intentionally does not re-check the active workspace. It is used only
/// for compensation after a keyring/file write may have completed while the
/// operation's generation was being cancelled.
pub(super) fn delete_secrets_at_target(target: &CapturedCredentialTarget) -> Result<(), String> {
    #[cfg(feature = "system-keyring")]
    {
        SecretStore::shared(keyring_service()).delete(&target.key)
    }
    #[cfg(not(feature = "system-keyring"))]
    {
        match fs::remove_file(&target.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("failed to remove saved credentials: {error}")),
        }
    }
}

pub(super) fn validate_connection_input(
    name: &str,
    provider: &str,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err("Give this connection a short name.".to_string());
    }
    if provider.trim().is_empty() || provider.len() > MAX_PROVIDER_BYTES {
        return Err("Name the service this connection uses.".to_string());
    }
    if command.trim().is_empty()
        || command.len() > MAX_COMMAND_BYTES
        || command.contains('\0')
        || command.contains('\n')
    {
        return Err("Enter a valid MCP server executable path.".to_string());
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
        if !super::super::is_well_formed_env_key(key) || super::super::is_reserved_env_key(key) {
            return Err(format!(
                "'{key}' cannot be used as a connection secret name."
            ));
        }
        if value.is_empty() {
            return Err(format!("Enter a value for '{key}' or remove it."));
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

pub(super) fn health_for_display(
    mut connection: StoredProjectConnection,
) -> StoredProjectConnection {
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

pub(super) fn find_connection<'a>(
    store: &'a ProjectConnectionStore,
    project_scope: &ProjectConnectionScope,
    connection_id: &str,
) -> Result<&'a StoredProjectConnection, String> {
    find_connection_index(store, project_scope, connection_id)
        .map(|index| &store.connections[index])
        .ok_or_else(|| "This connection no longer exists in this Project.".to_string())
}

pub(super) fn find_connection_index(
    store: &ProjectConnectionStore,
    project_scope: &ProjectConnectionScope,
    connection_id: &str,
) -> Option<usize> {
    store.connections.iter().position(|connection| {
        connection.id == connection_id && connection.project_scope == *project_scope
    })
}

pub(super) fn project_connection_count(
    store: &ProjectConnectionStore,
    project_scope: &ProjectConnectionScope,
) -> usize {
    store
        .connections
        .iter()
        .filter(|connection| connection.project_scope == *project_scope)
        .count()
}
