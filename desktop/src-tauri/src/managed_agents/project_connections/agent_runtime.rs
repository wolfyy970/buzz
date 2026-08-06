use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use sha2::Digest as _;
use tauri::AppHandle;

use super::{
    canonical_project_scope, capture_project_scope_for_app, connection_mcp_server_name,
    find_connection, health_for_display, load_secrets, load_store_unlocked,
    lock_project_connections, probe::approved_execution_target, workspace_connection_dir,
    CapturedProjectConnectionScope, ProjectConnection, ProjectConnectionHealthStatus,
    ProjectConnectionScope, StoredProjectConnection,
};
use crate::managed_agents::{
    validate_agent_project_scope, validate_tool_requirements, AgentDefinition, AgentProjectScope,
    AgentToolRequirement, BackendKind, ManagedAgentRecord,
};

pub(crate) fn canonical_agent_project_scope_for_app<R: tauri::Runtime>(
    app: &AppHandle<R>,
    scope: &AgentProjectScope,
) -> Result<AgentProjectScope, String> {
    let channel_id = uuid::Uuid::parse_str(&scope.channel_id)
        .map_err(|_| "Choose a valid Project discussion channel.".to_string())?
        .to_string();
    let canonical =
        capture_project_scope_for_app(app, &ProjectConnectionScope::from(scope))?.project;
    Ok(AgentProjectScope {
        relay_url: canonical.relay_url,
        operator_pubkey: canonical.operator_pubkey,
        project_address: canonical.project_address,
        channel_id,
    })
}

pub(crate) fn prepare_agent_project_assignment<R: tauri::Runtime>(
    app: &AppHandle<R>,
    definition: Option<&AgentDefinition>,
    requested_scope: Option<&AgentProjectScope>,
) -> Result<(Vec<AgentToolRequirement>, Option<AgentProjectScope>), String> {
    let requirements = definition
        .map(|definition| definition.tool_requirements.clone())
        .unwrap_or_default();
    validate_tool_requirements(&requirements)?;
    let scope = requested_scope
        .map(|scope| canonical_agent_project_scope_for_app(app, scope))
        .transpose()?;
    Ok((requirements, scope))
}

pub(crate) fn apply_agent_project_connection_update<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: &mut ManagedAgentRecord,
    project_scope: Option<Option<AgentProjectScope>>,
    connection_bindings: Option<BTreeMap<String, String>>,
) -> Result<(), String> {
    if let Some(scope) = project_scope {
        record.project_scope = scope
            .as_ref()
            .map(|scope| canonical_agent_project_scope_for_app(app, scope))
            .transpose()?;
    }
    if let Some(bindings) = connection_bindings {
        record.connection_bindings = bindings;
    }
    validate_agent_project_connections(app, record)
}

fn validate_agent_bindings_against(
    requirements: &[AgentToolRequirement],
    project_scope: Option<&AgentProjectScope>,
    bindings: &BTreeMap<String, String>,
    connections: &[ProjectConnection],
) -> Result<(), String> {
    validate_tool_requirements(requirements)?;
    if let Some(scope) = project_scope {
        validate_agent_project_scope(scope)?;
        if canonical_project_scope(&ProjectConnectionScope::from(scope))?
            != ProjectConnectionScope::from(scope)
        {
            return Err("The agent Project assignment is not canonical.".to_string());
        }
    }

    let requirement_by_id: BTreeMap<_, _> = requirements
        .iter()
        .map(|requirement| (requirement.id.as_str(), requirement))
        .collect();
    for requirement_id in bindings.keys() {
        if !requirement_by_id.contains_key(requirement_id.as_str()) {
            return Err(format!(
                "Connection binding {:?} does not match a tool requirement.",
                requirement_id
            ));
        }
    }
    for requirement in requirements {
        let Some(connection_id) = bindings.get(&requirement.id) else {
            if requirement.required {
                return Err(format!(
                    "Choose a Project connection for {}.",
                    requirement.label
                ));
            }
            continue;
        };
        let scope = project_scope.ok_or_else(|| {
            "Choose the Project where this agent will use its connections.".to_string()
        })?;
        let expected_scope = ProjectConnectionScope::from(scope);
        let connection = connections
            .iter()
            .find(|connection| {
                connection.id == *connection_id && connection.project_scope == expected_scope
            })
            .ok_or_else(|| {
                format!(
                    "The connection selected for {} no longer exists in this Project.",
                    requirement.label
                )
            })?;
        if connection.health.status != ProjectConnectionHealthStatus::Ready {
            return Err(format!(
                "Test {} again before using it with this agent.",
                connection.name
            ));
        }
        if !connection
            .capability_ids
            .iter()
            .any(|capability| capability == &requirement.capability)
        {
            return Err(format!(
                "{} does not provide the capability required by {}.",
                connection.name, requirement.label
            ));
        }
    }
    Ok(())
}

fn connections_for_scope<R: tauri::Runtime>(
    app: &AppHandle<R>,
    scope: &AgentProjectScope,
) -> Result<(CapturedProjectConnectionScope, Vec<StoredProjectConnection>), String> {
    let captured = capture_project_scope_for_app(app, &ProjectConnectionScope::from(scope))?;
    let store = load_store_unlocked(app, &captured)?;
    let connections = store
        .connections
        .into_iter()
        .filter(|connection| connection.project_scope == captured.project)
        .collect();
    Ok((captured, connections))
}

pub(crate) fn validate_agent_project_connections<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: &ManagedAgentRecord,
) -> Result<(), String> {
    if record.backend != BackendKind::Local
        && (!record.pinned_tool_requirements.is_empty()
            || !record.connection_bindings.is_empty()
            || record.project_scope.is_some())
    {
        return Err(
            "Project Connections are currently available only to agents running on this device."
                .to_string(),
        );
    }
    validate_tool_requirements(&record.pinned_tool_requirements)?;
    if record.project_scope.is_none() {
        return validate_agent_bindings_against(
            &record.pinned_tool_requirements,
            None,
            &record.connection_bindings,
            &[],
        );
    }

    let Some(scope) = record.project_scope.as_ref() else {
        return Err("Choose the Project where this agent will use its connections.".to_string());
    };
    let _guard = lock_project_connections();
    let (_, stored) = connections_for_scope(app, scope)?;
    let public: Vec<_> = stored
        .into_iter()
        .map(health_for_display)
        .map(ProjectConnection::from)
        .collect();
    validate_agent_bindings_against(
        &record.pinned_tool_requirements,
        Some(scope),
        &record.connection_bindings,
        &public,
    )
}

fn materialized_server<R: tauri::Runtime>(
    app: &AppHandle<R>,
    captured_scope: &CapturedProjectConnectionScope,
    connection: &StoredProjectConnection,
) -> Result<buzz_core_pkg::mcp_config::ConfiguredMcpServer, String> {
    let approved = approved_execution_target(captured_scope, connection)?;
    Ok(buzz_core_pkg::mcp_config::ConfiguredMcpServer::Stdio {
        name: connection_mcp_server_name(&connection.id),
        command: approved.command.to_string_lossy().to_string(),
        args: approved.args,
        env: load_secrets(app, captured_scope, connection)?,
    })
}

fn serialize_mcp_config(
    servers: Vec<buzz_core_pkg::mcp_config::ConfiguredMcpServer>,
    legacy_mcp_command: Option<&str>,
) -> Result<Vec<u8>, String> {
    let legacy_count = usize::from(legacy_mcp_command.is_some_and(|command| !command.is_empty()));
    if servers.len() + legacy_count > buzz_core_pkg::mcp_config::MCP_SERVER_MAX_COUNT {
        return Err(format!(
            "Project connections exceed the agent runtime limit of {} MCP servers.",
            buzz_core_pkg::mcp_config::MCP_SERVER_MAX_COUNT
        ));
    }
    let legacy_name = legacy_mcp_command
        .filter(|command| !command.is_empty())
        .and_then(|command| Path::new(command).file_stem())
        .and_then(|name| name.to_str())
        .unwrap_or("mcp");
    let mut server_names = BTreeSet::new();
    for server in &servers {
        let buzz_core_pkg::mcp_config::ConfiguredMcpServer::Stdio { name, env, .. } = server;
        if !server_names.insert(name.as_str()) || (legacy_count != 0 && name == legacy_name) {
            return Err("Project connections have colliding MCP server names.".to_string());
        }
        let mut normalized_env_keys = BTreeSet::new();
        if env
            .keys()
            .any(|key| !normalized_env_keys.insert(key.to_ascii_uppercase()))
        {
            return Err(format!(
                "Project connection {} has duplicate secret names.",
                name
            ));
        }
    }
    buzz_core_pkg::mcp_config::McpLaunchConfigDocument::new(servers)
        .to_json()
        .map_err(|error| format!("failed to prepare Project connections: {error}"))
}

fn validate_session_tool_count(counts: impl IntoIterator<Item = usize>) -> Result<(), String> {
    let total = counts
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or_else(|| "Project connections expose too many tools.".to_string())?;
    if total > buzz_agent_pkg::MAX_MCP_TOOLS_PER_SESSION {
        return Err(format!(
            "Project connections exceed the bundled agent limit of {} tools.",
            buzz_agent_pkg::MAX_MCP_TOOLS_PER_SESSION
        ));
    }
    Ok(())
}

pub(crate) fn materialize_agent_project_connections<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: &ManagedAgentRecord,
    legacy_mcp_command: Option<&str>,
) -> Result<Option<Vec<u8>>, String> {
    if record.backend != BackendKind::Local
        && (!record.pinned_tool_requirements.is_empty()
            || !record.connection_bindings.is_empty()
            || record.project_scope.is_some())
    {
        return Err(
            "Project Connections are currently available only to agents running on this device."
                .to_string(),
        );
    }
    validate_tool_requirements(&record.pinned_tool_requirements)?;
    let Some(scope) = record.project_scope.as_ref() else {
        validate_agent_bindings_against(
            &record.pinned_tool_requirements,
            None,
            &record.connection_bindings,
            &[],
        )?;
        return Ok(None);
    };
    if record.connection_bindings.is_empty() {
        validate_agent_bindings_against(
            &record.pinned_tool_requirements,
            Some(scope),
            &record.connection_bindings,
            &[],
        )?;
        return Ok(None);
    }
    let _guard = lock_project_connections();
    let (captured, store_connections) = connections_for_scope(app, scope)?;
    let public: Vec<_> = store_connections
        .iter()
        .cloned()
        .map(health_for_display)
        .map(ProjectConnection::from)
        .collect();
    validate_agent_bindings_against(
        &record.pinned_tool_requirements,
        Some(scope),
        &record.connection_bindings,
        &public,
    )?;
    let store = super::ProjectConnectionStore {
        version: super::CONNECTION_STORE_VERSION,
        connections: store_connections,
    };
    let connection_ids: BTreeSet<_> = record.connection_bindings.values().cloned().collect();
    let selected_connections = connection_ids
        .iter()
        .map(|connection_id| find_connection(&store, &captured.project, connection_id))
        .collect::<Result<Vec<_>, _>>()?;
    validate_session_tool_count(
        selected_connections
            .iter()
            .map(|connection| connection.discovered_tools.len()),
    )?;
    let mut servers = Vec::with_capacity(connection_ids.len());
    for connection in selected_connections {
        servers.push(materialized_server(app, &captured, connection)?);
    }
    serialize_mcp_config(servers, legacy_mcp_command).map(Some)
}

pub(crate) fn write_agent_project_connection_config<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: &ManagedAgentRecord,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let scope = record.project_scope.as_ref().ok_or_else(|| {
        "Choose the Project where this agent will use its connections.".to_string()
    })?;
    let captured = capture_project_scope_for_app(app, &ProjectConnectionScope::from(scope))?;
    let runtime_dir = workspace_connection_dir(&captured)?.join("runtime");
    super::ensure_owner_only_directory(&runtime_dir)?;
    let agent_digest = sha2::Sha256::digest(record.pubkey.as_bytes());
    let path = runtime_dir.join(format!(
        "agent-{}-{}.json",
        hex::encode(&agent_digest[..8]),
        uuid::Uuid::new_v4().simple()
    ));
    super::reject_unsafe_owner_file(&path)?;
    crate::managed_agents::atomic_write_json_restricted(&path, bytes)?;
    Ok(path)
}

pub(crate) fn remove_agent_project_connection_config(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to remove Project connection launch file {}: {error}",
            path.display()
        )),
    }
}

pub(crate) fn remove_stale_agent_project_connection_configs(
    captured_scope: &CapturedProjectConnectionScope,
) -> Result<(), String> {
    let runtime_dir = workspace_connection_dir(captured_scope)?.join("runtime");
    let entries = match fs::read_dir(&runtime_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to inspect stale Project connection launch files: {error}"
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("failed to inspect stale Project connection launch file: {error}")
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("agent-") || !name.ends_with(".json") {
            continue;
        }
        let path = entry.path();
        super::reject_unsafe_owner_file(&path)?;
        remove_agent_project_connection_config(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_agents::project_connections::ProjectConnectionHealth;

    fn scope(channel_id: &str) -> AgentProjectScope {
        AgentProjectScope {
            relay_url: "ws://127.0.0.1:3000".to_string(),
            operator_pubkey: "a".repeat(64),
            project_address: format!("30621:{}:analytics", "a".repeat(64)),
            channel_id: channel_id.to_string(),
        }
    }

    fn requirement(id: &str, capability: &str, required: bool) -> AgentToolRequirement {
        AgentToolRequirement {
            id: id.to_string(),
            label: "Analytics".to_string(),
            capability: capability.to_string(),
            required,
        }
    }

    fn connection(scope: &AgentProjectScope, id: &str) -> ProjectConnection {
        ProjectConnection {
            id: id.to_string(),
            project_scope: ProjectConnectionScope::from(scope),
            name: "Analytics connection".to_string(),
            provider: "Local".to_string(),
            capability_ids: vec!["mcp.tool.analytics_weekly_summary".to_string()],
            command: "/usr/bin/true".to_string(),
            args: Vec::new(),
            env_keys: Vec::new(),
            discovered_tools: vec!["analytics_weekly_summary".to_string()],
            health: ProjectConnectionHealth {
                status: ProjectConnectionHealthStatus::Ready,
                last_verified_at: Some(crate::util::now_iso()),
                detail: None,
            },
            created_at: crate::util::now_iso(),
            updated_at: crate::util::now_iso(),
        }
    }

    fn materialized_server_for_test(
        index: usize,
    ) -> buzz_core_pkg::mcp_config::ConfiguredMcpServer {
        buzz_core_pkg::mcp_config::ConfiguredMcpServer::Stdio {
            name: format!("project_{index:032x}"),
            command: "/usr/bin/true".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn required_optional_orphan_scope_health_and_capability_are_enforced() {
        let scope = scope(&uuid::Uuid::nil().to_string());
        let connection_id = "c".repeat(32);
        let required = requirement("analytics", "mcp.tool.analytics_weekly_summary", true);
        assert!(validate_agent_bindings_against(
            std::slice::from_ref(&required),
            Some(&scope),
            &BTreeMap::new(),
            &[]
        )
        .is_err());

        let optional = requirement("analytics", "mcp.tool.analytics_weekly_summary", false);
        assert!(validate_agent_bindings_against(&[optional], None, &BTreeMap::new(), &[]).is_ok());

        let bindings = BTreeMap::from([("unknown".to_string(), connection_id.clone())]);
        assert!(validate_agent_bindings_against(
            std::slice::from_ref(&required),
            Some(&scope),
            &bindings,
            &[],
        )
        .is_err());

        let bindings = BTreeMap::from([("analytics".to_string(), connection_id.clone())]);
        let mut wrong_scope = scope.clone();
        wrong_scope.project_address = format!("30621:{}:other", "a".repeat(64));
        assert!(validate_agent_bindings_against(
            std::slice::from_ref(&required),
            Some(&wrong_scope),
            &bindings,
            &[connection(&scope, &connection_id)],
        )
        .is_err());

        let mut unavailable = connection(&scope, &connection_id);
        unavailable.health.status = ProjectConnectionHealthStatus::CheckNeeded;
        assert!(validate_agent_bindings_against(
            std::slice::from_ref(&required),
            Some(&scope),
            &bindings,
            &[unavailable],
        )
        .is_err());

        let wrong_capability = requirement("analytics", "mcp.tool.analytics.delete_all", true);
        assert!(validate_agent_bindings_against(
            &[wrong_capability],
            Some(&scope),
            &bindings,
            &[connection(&scope, &connection_id)],
        )
        .is_err());

        assert!(validate_agent_bindings_against(
            &[required],
            Some(&scope),
            &bindings,
            &[connection(&scope, &connection_id)],
        )
        .is_ok());
    }

    #[test]
    fn one_connection_can_satisfy_multiple_requirements_once() {
        let scope = scope(&uuid::Uuid::nil().to_string());
        let connection_id = "c".repeat(32);
        let connection = connection(&scope, &connection_id);
        let requirements = vec![
            requirement("weekly", "mcp.tool.analytics_weekly_summary", true),
            requirement("monthly", "mcp.tool.analytics_weekly_summary", true),
        ];
        let bindings = BTreeMap::from([
            ("weekly".to_string(), connection_id.clone()),
            ("monthly".to_string(), connection_id),
        ]);
        assert!(validate_agent_bindings_against(
            &requirements,
            Some(&scope),
            &bindings,
            &[connection],
        )
        .is_ok());
        assert_eq!(bindings.values().collect::<BTreeSet<_>>().len(), 1,);
    }

    #[test]
    fn materialized_config_enforces_harness_server_and_size_limits() {
        let sixteen = (0..16)
            .map(materialized_server_for_test)
            .collect::<Vec<_>>();
        assert!(serialize_mcp_config(sixteen, None).is_ok());

        let sixteen_with_legacy = (0..16)
            .map(materialized_server_for_test)
            .collect::<Vec<_>>();
        assert!(serialize_mcp_config(sixteen_with_legacy, Some("/usr/bin/legacy")).is_err());

        let mut oversized = materialized_server_for_test(0);
        let buzz_core_pkg::mcp_config::ConfiguredMcpServer::Stdio { args, .. } = &mut oversized;
        args.push("x".repeat(64 * 1024));
        assert!(serialize_mcp_config(vec![oversized], None).is_err());
    }

    #[test]
    fn materialized_config_rejects_case_colliding_environment_names() {
        let mut server = materialized_server_for_test(0);
        let buzz_core_pkg::mcp_config::ConfiguredMcpServer::Stdio { env, .. } = &mut server;
        *env = BTreeMap::from([
            ("API_TOKEN".to_string(), "one".to_string()),
            ("api_token".to_string(), "two".to_string()),
        ]);
        assert!(serialize_mcp_config(vec![server], None).is_err());
    }

    #[test]
    fn materialized_config_rejects_structured_and_legacy_server_name_collisions() {
        let server = materialized_server_for_test(0);
        let buzz_core_pkg::mcp_config::ConfiguredMcpServer::Stdio { name, .. } = &server;
        let legacy = format!("/usr/local/bin/{name}");
        assert!(serialize_mcp_config(vec![server], Some(&legacy)).is_err());
    }

    #[test]
    fn selected_project_tools_fit_the_bundled_session_count_contract() {
        assert!(validate_session_tool_count([64, 64]).is_ok());
        assert!(validate_session_tool_count([128, 1]).is_err());
    }
}
