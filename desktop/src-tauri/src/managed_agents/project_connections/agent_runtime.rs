use std::{
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io::{BufRead as _, BufReader, Write as _},
    process::{Command, Stdio},
    time::Duration,
};

use super::*;
use crate::managed_agents::{BackendKind, ManagedAgentRecord};

const TEST_TIMEOUT: Duration = Duration::from_secs(8);

fn inherited_test_env() -> BTreeMap<String, String> {
    [
        "PATH",
        "HOME",
        "USER",
        "TMPDIR",
        "TEMP",
        "TMP",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
    ]
    .into_iter()
    .filter_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| (key.to_string(), value))
    })
    .collect()
}

fn recv_json_response(
    rx: &std::sync::mpsc::Receiver<String>,
    expected_id: u64,
) -> Result<serde_json::Value, String> {
    let deadline = std::time::Instant::now() + TEST_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let line = rx
            .recv_timeout(remaining)
            .map_err(|_| "The MCP server did not respond in time.".to_string())?;
        if line.len() > 1024 * 1024 {
            return Err("The MCP server returned an oversized response.".to_string());
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .map_err(|_| "The MCP server returned an invalid response.".to_string())?;
        if value.get("id").and_then(serde_json::Value::as_u64) == Some(expected_id) {
            if let Some(error) = value.get("error") {
                let message = error
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("The MCP server rejected the request.");
                return Err(message.to_string());
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| "The MCP server returned no result.".to_string());
        }
    }
}

pub(super) fn probe_mcp_connection(
    connection: &ProjectConnection,
    secrets: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let executable = resolve_command(&connection.command)
        .ok_or_else(|| format!("Buzz could not find '{}'.", connection.command))?;
    let mut command = Command::new(executable);
    command
        .args(&connection.args)
        .env_clear()
        .envs(inherited_test_env())
        .envs(secrets)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Buzz could not start this MCP server.".to_string())?;
    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Buzz could not read from this MCP server.".to_string())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Buzz could not write to this MCP server.".to_string())?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let result = (|| {
        let initialize = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "buzz-desktop",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });
        writeln!(stdin, "{initialize}")
            .and_then(|_| stdin.flush())
            .map_err(|_| "Buzz could not initialize this MCP server.".to_string())?;
        let initialized = recv_json_response(&rx, 1)?;
        if initialized.get("protocolVersion").is_none() {
            return Err("The MCP server did not complete initialization.".to_string());
        }
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            })
        )
        .and_then(|_| {
            writeln!(
                stdin,
                "{}",
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list",
                    "params": {}
                })
            )
        })
        .and_then(|_| stdin.flush())
        .map_err(|_| "Buzz could not inspect this MCP server.".to_string())?;
        let tools_result = recv_json_response(&rx, 2)?;
        let tools = tools_result
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "The MCP server did not return a tool list.".to_string())?;
        let mut names = Vec::with_capacity(tools.len());
        for tool in tools {
            let name = tool
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| valid_stable_id(name, 128))
                .ok_or_else(|| "The MCP server returned an invalid tool name.".to_string())?;
            names.push(name.to_string());
        }
        names.sort();
        names.dedup();
        Ok(names)
    })();

    drop(stdin);
    let _ = super::super::runtime::terminate_process(pid);
    let _ = child.wait();
    result
}

pub fn test_project_connection(
    app: &AppHandle,
    connection_id: &str,
) -> Result<ProjectConnection, String> {
    let connection = {
        let _guard = lock_project_connections();
        load_store_unlocked(app)?
            .connections
            .into_iter()
            .find(|connection| connection.id == connection_id)
            .ok_or_else(|| "This connection no longer exists.".to_string())?
    };
    validate_project_scope_for_app(app, &connection.project_scope)?;
    let secrets = match load_secrets(&connection) {
        Ok(secrets) => secrets,
        Err(error) => {
            let _guard = lock_project_connections();
            let mut store = load_store_unlocked(app)?;
            if let Some(current) = store.connections.iter_mut().find(|candidate| {
                candidate.id == connection.id && candidate.generation == connection.generation
            }) {
                current.updated_at = now_iso();
                current.health = ProjectConnectionHealth {
                    status: ProjectConnectionHealthStatus::SignInRequired,
                    last_verified_at: None,
                    detail: Some(error.clone()),
                };
                save_store_unlocked(app, &store)?;
            }
            return Err(error);
        }
    };
    let result = probe_mcp_connection(&connection, &secrets);
    let _guard = lock_project_connections();
    let mut store = load_store_unlocked(app)?;
    let index = store
        .connections
        .iter()
        .position(|candidate| candidate.id == connection_id)
        .ok_or_else(|| "This connection was removed while Buzz tested it.".to_string())?;
    if store.connections[index].generation != connection.generation {
        return Err("This connection changed while Buzz tested it. Test it again.".to_string());
    }
    let connection = &mut store.connections[index];
    connection.updated_at = now_iso();
    match result {
        Ok(tools) => {
            connection.discovered_tools = tools.clone();
            connection.capability_ids = tools
                .iter()
                .map(|tool| format!("mcp.tool.{tool}"))
                .collect();
            connection.health = ProjectConnectionHealth {
                status: ProjectConnectionHealthStatus::Ready,
                last_verified_at: Some(now_iso()),
                detail: None,
            };
            let updated = connection.clone();
            save_store_unlocked(app, &store)?;
            Ok(updated)
        }
        Err(error) => {
            connection.health = ProjectConnectionHealth {
                status: ProjectConnectionHealthStatus::Unavailable,
                last_verified_at: None,
                detail: Some(error.clone()),
            };
            let _ = save_store_unlocked(app, &store);
            Err(error)
        }
    }
}

pub(super) fn validate_agent_bindings_against(
    record: &ManagedAgentRecord,
    connections: &[ProjectConnection],
) -> Result<Vec<ProjectConnection>, String> {
    validate_tool_requirements(&record.pinned_tool_requirements)?;
    if record.pinned_tool_requirements.is_empty() {
        if !record.connection_bindings.is_empty() {
            return Err(
                "Remove the unused connection bindings before starting this agent.".to_string(),
            );
        }
        return Ok(Vec::new());
    }
    if record.backend != BackendKind::Local {
        return Err(
            "Project Connections are available for agents running on this device. Choose This device to continue."
                .to_string(),
        );
    }
    let scope = record
        .project_scope
        .as_ref()
        .ok_or_else(|| "Choose the Project where this agent will work.".to_string())?;
    validate_project_scope(scope)?;

    let requirements_by_id: BTreeMap<_, _> = record
        .pinned_tool_requirements
        .iter()
        .map(|requirement| (requirement.id.as_str(), requirement))
        .collect();
    if record
        .connection_bindings
        .keys()
        .any(|id| !requirements_by_id.contains_key(id.as_str()))
    {
        return Err("This agent has a connection for a tool it no longer requires.".to_string());
    }

    let mut selected = BTreeMap::<String, ProjectConnection>::new();
    for requirement in &record.pinned_tool_requirements {
        let Some(connection_id) = record.connection_bindings.get(&requirement.id) else {
            if requirement.required {
                return Err(format!(
                    "Choose a connection for {} before starting this agent.",
                    requirement.label
                ));
            }
            continue;
        };
        let connection = connections
            .iter()
            .find(|connection| connection.id == *connection_id)
            .ok_or_else(|| {
                format!(
                    "The connection selected for {} no longer exists.",
                    requirement.label
                )
            })?;
        if connection.project_scope != *scope {
            return Err(format!(
                "The connection selected for {} belongs to another Project.",
                requirement.label
            ));
        }
        let displayed = health_for_display(connection.clone());
        if displayed.health.status != ProjectConnectionHealthStatus::Ready {
            return Err(format!(
                "Test '{}' before starting this agent.",
                connection.name
            ));
        }
        if !connection
            .capability_ids
            .iter()
            .any(|capability| capability == &requirement.capability)
        {
            return Err(format!(
                "'{}' cannot provide {}.",
                connection.name, requirement.label
            ));
        }
        selected
            .entry(connection.id.clone())
            .or_insert_with(|| connection.clone());
    }
    Ok(selected.into_values().collect())
}

fn binding_issues_against(
    record: &ManagedAgentRecord,
    connections: &[ProjectConnection],
) -> Result<Vec<AgentToolBindingIssue>, String> {
    validate_tool_requirements(&record.pinned_tool_requirements)?;
    let mut issues = Vec::new();
    let requirements_by_id: BTreeMap<_, _> = record
        .pinned_tool_requirements
        .iter()
        .map(|requirement| (requirement.id.as_str(), requirement))
        .collect();
    for id in record
        .connection_bindings
        .keys()
        .filter(|id| !requirements_by_id.contains_key(id.as_str()))
    {
        issues.push(AgentToolBindingIssue {
            requirement_id: id.clone(),
            label: id.clone(),
            reason: "This tool is no longer required. Remove its connection.".to_string(),
        });
    }
    if record.pinned_tool_requirements.is_empty() {
        return Ok(issues);
    }
    if record.backend != BackendKind::Local {
        for requirement in &record.pinned_tool_requirements {
            issues.push(AgentToolBindingIssue {
                requirement_id: requirement.id.clone(),
                label: requirement.label.clone(),
                reason: "Project Connections currently require an agent running on this device."
                    .to_string(),
            });
        }
        return Ok(issues);
    }
    let Some(scope) = record.project_scope.as_ref() else {
        for requirement in record
            .pinned_tool_requirements
            .iter()
            .filter(|requirement| requirement.required)
        {
            issues.push(AgentToolBindingIssue {
                requirement_id: requirement.id.clone(),
                label: requirement.label.clone(),
                reason: "Choose the Project where this agent will work.".to_string(),
            });
        }
        return Ok(issues);
    };
    validate_project_scope(scope)?;
    for requirement in &record.pinned_tool_requirements {
        let Some(connection_id) = record.connection_bindings.get(&requirement.id) else {
            if requirement.required {
                issues.push(AgentToolBindingIssue {
                    requirement_id: requirement.id.clone(),
                    label: requirement.label.clone(),
                    reason: format!("Choose a connection for {}.", requirement.label),
                });
            }
            continue;
        };
        let Some(connection) = connections
            .iter()
            .find(|connection| connection.id == *connection_id)
        else {
            issues.push(AgentToolBindingIssue {
                requirement_id: requirement.id.clone(),
                label: requirement.label.clone(),
                reason: "The selected connection no longer exists.".to_string(),
            });
            continue;
        };
        let reason = if connection.project_scope != *scope {
            Some("The selected connection belongs to another Project.".to_string())
        } else if health_for_display(connection.clone()).health.status
            != ProjectConnectionHealthStatus::Ready
        {
            Some(format!("Test '{}' before using it.", connection.name))
        } else if !connection
            .capability_ids
            .iter()
            .any(|capability| capability == &requirement.capability)
        {
            Some(format!(
                "'{}' cannot provide {}.",
                connection.name, requirement.label
            ))
        } else {
            None
        };
        if let Some(reason) = reason {
            issues.push(AgentToolBindingIssue {
                requirement_id: requirement.id.clone(),
                label: requirement.label.clone(),
                reason,
            });
        }
    }
    Ok(issues)
}

pub(crate) fn agent_tool_binding_issues(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<Vec<AgentToolBindingIssue>, String> {
    if let Some(scope) = &record.project_scope {
        validate_project_scope_for_app(app, scope)?;
    }
    let _guard = lock_project_connections();
    binding_issues_against(record, &load_store_unlocked(app)?.connections)
}

pub(crate) fn validate_agent_project_connections(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<(), String> {
    if let Some(scope) = &record.project_scope {
        validate_project_scope_for_app(app, scope)?;
    }
    let _guard = lock_project_connections();
    let connections = load_store_unlocked(app)?.connections;
    validate_agent_bindings_against(record, &connections).map(|_| ())
}

pub(crate) fn materialize_agent_project_connections(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<Option<MaterializedProjectConnections>, String> {
    if let Some(scope) = &record.project_scope {
        validate_project_scope_for_app(app, scope)?;
    }
    let _guard = lock_project_connections();
    let connections = load_store_unlocked(app)?.connections;
    let selected = validate_agent_bindings_against(record, &connections)?;
    if selected.is_empty() {
        return Ok(None);
    }
    let mut hasher = DefaultHasher::new();
    let mut servers = Vec::with_capacity(selected.len());
    for connection in selected {
        connection.id.hash(&mut hasher);
        connection.generation.hash(&mut hasher);
        let env = load_secrets(&connection)?;
        servers.push(MaterializedMcpServer {
            name: format!("project_{}", connection.id),
            command: connection.command,
            args: connection.args,
            env,
        });
    }
    let document = MaterializedMcpDocument {
        version: 1,
        servers,
    };
    let json = serde_json::to_vec(&document)
        .map_err(|error| format!("failed to prepare Project connections: {error}"))?;
    Ok(Some(MaterializedProjectConnections {
        json,
        generation_hash: hasher.finish(),
    }))
}

pub(crate) fn current_connection_generation_hash(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<u64, String> {
    if let Some(scope) = &record.project_scope {
        validate_project_scope_for_app(app, scope)?;
    }
    let _guard = lock_project_connections();
    let selected = validate_agent_bindings_against(record, &load_store_unlocked(app)?.connections)?;
    if selected.is_empty() {
        return Ok(0);
    }
    let mut hasher = DefaultHasher::new();
    for connection in selected {
        connection.id.hash(&mut hasher);
        connection.generation.hash(&mut hasher);
    }
    Ok(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use uuid::Uuid;

    #[test]
    fn synthetic_analytics_server_proves_initialize_and_tool_discovery() {
        let Some(node) = resolve_command("node") else {
            eprintln!("node is unavailable; skipping synthetic MCP probe");
            return;
        };
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/synthetic-analytics-mcp.mjs");
        assert!(script.is_file(), "missing fixture {}", script.display());

        let scope = AgentProjectScope {
            relay_url: "ws://127.0.0.1:3000".to_string(),
            operator_pubkey: "a".repeat(64),
            repo_address: format!("30617:{}:portable-agents", "a".repeat(64)),
            channel_id: Uuid::nil().to_string(),
        };
        let connection = ProjectConnection {
            id: "e007-synthetic-analytics".to_string(),
            project_scope: scope,
            name: "Synthetic analytics".to_string(),
            provider: "Buzz test fixture".to_string(),
            capability_ids: Vec::new(),
            command: node.to_string_lossy().to_string(),
            args: vec![script.to_string_lossy().to_string()],
            env_keys: vec!["E007_ANALYTICS_CANARY".to_string()],
            discovered_tools: Vec::new(),
            health: ProjectConnectionHealth::default(),
            generation: 1,
            credential_generation: 1,
            created_at: now_iso(),
            updated_at: now_iso(),
        };
        let secrets =
            BTreeMap::from([("E007_ANALYTICS_CANARY".to_string(), "test-only".to_string())]);

        assert_eq!(
            probe_mcp_connection(&connection, &secrets).unwrap(),
            ["analytics.weekly_summary"]
        );
    }
}
