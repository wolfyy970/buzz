use super::*;
use crate::managed_agents::{BackendKind, ManagedAgentRecord};

fn scope() -> AgentProjectScope {
    AgentProjectScope {
        relay_url: "ws://127.0.0.1:3000".to_string(),
        operator_pubkey: "b".repeat(64),
        repo_address: format!("30617:{}:portable-agents", "a".repeat(64)),
        channel_id: Uuid::nil().to_string(),
    }
}

fn requirement(required: bool) -> AgentToolRequirement {
    AgentToolRequirement {
        id: "analytics".to_string(),
        label: "Analytics reports".to_string(),
        capability: "mcp.tool.run_report".to_string(),
        required,
    }
}

fn record(required: bool) -> ManagedAgentRecord {
    let definition = crate::managed_agents::AgentDefinition {
        id: "analytics".to_string(),
        display_name: "Analytics".to_string(),
        avatar_url: None,
        system_prompt: "Analyze the data.".to_string(),
        runtime: Some("codex".to_string()),
        model: None,
        provider: None,
        name_pool: Vec::new(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: BTreeMap::new(),
        respond_to: None,
        respond_to_allowlist: Vec::new(),
        parallelism: None,
        created_at: now_iso(),
        updated_at: now_iso(),
        tool_requirements: vec![requirement(required)],
    };
    let mut record = definition.into_agent_record();
    record.project_scope = Some(scope());
    record
}

fn connection() -> ProjectConnection {
    ProjectConnection {
        id: "connection-1".to_string(),
        project_scope: scope(),
        name: "Analytics".to_string(),
        provider: "Local test".to_string(),
        capability_ids: vec!["mcp.tool.run_report".to_string()],
        command: "/usr/bin/true".to_string(),
        args: Vec::new(),
        env_keys: Vec::new(),
        discovered_tools: vec!["run_report".to_string()],
        health: ProjectConnectionHealth {
            status: ProjectConnectionHealthStatus::Ready,
            last_verified_at: Some(now_iso()),
            detail: None,
        },
        generation: 1,
        credential_generation: 1,
        created_at: now_iso(),
        updated_at: now_iso(),
    }
}

#[test]
fn project_scope_requires_canonical_coordinate_and_channel() {
    assert_eq!(canonical_project_scope(&scope()).unwrap(), scope());
    assert!(validate_project_scope(&scope()).is_ok());
    let mut invalid = scope();
    invalid.repo_address = "local-project-id".to_string();
    assert!(validate_project_scope(&invalid).is_err());
    let mut invalid = scope();
    invalid.channel_id = "general".to_string();
    assert!(validate_project_scope(&invalid).is_err());
}

#[test]
fn tool_requirements_reject_duplicate_ids_and_non_tool_capabilities() {
    let requirement = requirement(true);
    assert!(validate_tool_requirements(&[requirement.clone()]).is_ok());
    assert!(validate_tool_requirements(&[requirement.clone(), requirement]).is_err());
    assert!(validate_tool_requirements(&[AgentToolRequirement {
        id: "analytics".to_string(),
        label: "Analytics reports".to_string(),
        capability: "analytics.reports.read".to_string(),
        required: true,
    }])
    .is_err());
}

#[test]
fn required_tools_block_until_a_ready_matching_connection_is_bound() {
    let mut record = record(true);
    let matching = connection();

    assert!(validate_agent_bindings_against(&record, &[matching.clone()]).is_err());

    record
        .connection_bindings
        .insert("analytics".to_string(), matching.id.clone());
    assert_eq!(
        validate_agent_bindings_against(&record, &[matching.clone()])
            .unwrap()
            .len(),
        1
    );

    let mut wrong_capability = matching.clone();
    wrong_capability.capability_ids = vec!["mcp.tool.other".to_string()];
    assert!(validate_agent_bindings_against(&record, &[wrong_capability]).is_err());

    let mut untested = matching;
    untested.health = ProjectConnectionHealth::default();
    assert!(validate_agent_bindings_against(&record, &[untested]).is_err());
}

#[test]
fn optional_tools_can_remain_unbound_but_remote_tools_are_rejected() {
    let mut record = record(false);
    assert!(validate_agent_bindings_against(&record, &[]).is_ok());
    record.backend = BackendKind::Provider {
        id: "remote".to_string(),
        config: serde_json::Value::Null,
    };
    assert!(validate_agent_bindings_against(&record, &[]).is_err());
}

#[test]
fn bindings_cannot_cross_project_boundaries() {
    let mut record = record(true);
    let mut connection = connection();
    record
        .connection_bindings
        .insert("analytics".to_string(), connection.id.clone());
    connection.project_scope.repo_address = format!("30617:{}:another-project", "a".repeat(64));

    assert!(validate_agent_bindings_against(&record, &[connection]).is_err());
}

#[cfg(unix)]
#[test]
fn connection_store_rejects_symlinks_and_non_owner_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};

    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("project-connections.json");
    let target = dir.path().join("target.json");
    fs::write(&target, b"{}").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, &store).unwrap();
    assert!(reject_unsafe_connection_store(&store).is_err());

    fs::remove_file(&store).unwrap();
    fs::write(&store, b"{}").unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(reject_unsafe_connection_store(&store).is_err());
    fs::set_permissions(&store, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(reject_unsafe_connection_store(&store).is_ok());
}
