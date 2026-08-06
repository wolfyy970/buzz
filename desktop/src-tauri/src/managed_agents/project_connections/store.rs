//! Bounded persistence and full validation for Project connection metadata.

use super::*;

pub(super) fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn validate_stored_connection(
    connection: &StoredProjectConnection,
) -> Result<(), String> {
    if !is_lower_hex(&connection.id, 32)
        || !is_lower_hex(&connection.generation, 32)
        || !is_lower_hex(&connection.credential_generation, 32)
        || !is_lower_hex(&connection.executable_sha256, 64)
        || canonical_project_scope(&connection.project_scope)? != connection.project_scope
        || !Path::new(&connection.command).is_absolute()
    {
        return Err("Project connection metadata is invalid.".to_string());
    }
    let placeholder_env = connection
        .env_keys
        .iter()
        .map(|key| (key.clone(), "stored".to_string()))
        .collect();
    validate_connection_input(
        &connection.name,
        &connection.provider,
        &connection.command,
        &connection.args,
        &placeholder_env,
    )
    .map_err(|_| "Project connection metadata is invalid.".to_string())?;
    if connection
        .env_keys
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
        || connection
            .discovered_tools
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || connection.discovered_tools.len() > buzz_agent_pkg::MAX_MCP_TOOLS_PER_SESSION
    {
        return Err("Project connection metadata is invalid.".to_string());
    }
    let server_name = connection_mcp_server_name(&connection.id);
    if connection
        .discovered_tools
        .iter()
        .any(|tool| !buzz_agent_pkg::supports_mcp_server_tool_name(&server_name, tool))
    {
        return Err("Project connection metadata is invalid.".to_string());
    }
    let expected_capabilities = connection
        .discovered_tools
        .iter()
        .map(|tool| format!("mcp.tool.{tool}"))
        .collect::<Vec<_>>();
    if connection.capability_ids != expected_capabilities {
        return Err("Project connection metadata is invalid.".to_string());
    }
    if chrono::DateTime::parse_from_rfc3339(&connection.created_at).is_err()
        || chrono::DateTime::parse_from_rfc3339(&connection.updated_at).is_err()
        || connection
            .health
            .last_verified_at
            .as_deref()
            .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_err())
        || connection
            .health
            .detail
            .as_deref()
            .is_some_and(|detail| detail.len() > 4_096 || detail.chars().any(char::is_control))
        || (connection.health.status == ProjectConnectionHealthStatus::Ready
            && (connection.health.last_verified_at.is_none()
                || connection.discovered_tools.is_empty()))
    {
        return Err("Project connection metadata is invalid.".to_string());
    }
    Ok(())
}
