use super::*;
use crate::managed_agents::discovery::known_acp_runtime_exact;

fn record() -> ManagedAgentRecord {
    ManagedAgentRecord {
        pubkey: "test-pubkey".to_string(),
        name: "test-agent".to_string(),
        persona_id: None,
        private_key_nsec: String::new(),
        auth_tag: None,
        relay_url: String::new(),
        avatar_url: None,
        acp_command: "buzz-acp".to_string(),
        agent_command: "buzz-agent".to_string(),
        agent_command_override: None,
        agent_args: vec![],
        mcp_command: String::new(),
        turn_timeout_seconds: 320,
        idle_timeout_seconds: None,
        max_turn_duration_seconds: None,
        parallelism: 1,
        system_prompt: None,
        model: None,
        provider: None,
        persona_source_version: None,
        pinned_persona_env_vars: None,
        previous_persona_snapshots: Vec::new(),
        env_vars: BTreeMap::new(),
        start_on_app_launch: false,
        auto_restart_on_config_change: true,
        runtime_pid: None,
        backend: Default::default(),
        backend_agent_id: None,
        provider_binary_path: None,
        team_id: None,
        persona_team_dir: None,
        persona_name_in_team: None,
        created_at: String::new(),
        updated_at: String::new(),
        last_started_at: None,
        last_stopped_at: None,
        last_exit_code: None,
        last_error: None,
        last_error_code: None,
        respond_to: Default::default(),
        respond_to_allowlist: vec![],
        display_name: None,
        slug: None,
        runtime: None,
        name_pool: Vec::new(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        definition_respond_to: None,
        definition_respond_to_allowlist: Vec::new(),
        definition_parallelism: None,
        relay_mesh: None,
        connection_bindings: std::collections::BTreeMap::new(),
        pinned_tool_requirements: Vec::new(),
        project_scope: None,
    }
}

#[test]
fn user_env_wins_over_structured_runtime_fields() {
    let mut record = record();
    record.env_vars = BTreeMap::from([
        ("BUZZ_AGENT_PROVIDER".to_string(), "anthropic".to_string()),
        (
            "BUZZ_AGENT_MODEL".to_string(),
            "claude-opus-4-5".to_string(),
        ),
    ]);
    let effective = resolve_effective_agent_env(
        &record,
        &[],
        known_acp_runtime_exact("buzz-agent"),
        &Default::default(),
    );
    assert_eq!(
        effective.env.get("BUZZ_AGENT_PROVIDER").map(String::as_str),
        Some("anthropic")
    );
    assert_eq!(
        effective.env.get("BUZZ_AGENT_MODEL").map(String::as_str),
        Some("claude-opus-4-5")
    );
}

#[test]
fn linked_uninitialized_env_fails_runtime_and_readiness_closed() {
    let mut record = record();
    record.persona_id = Some("analytics".to_string());

    assert_eq!(
        ensure_persona_env_snapshot_initialized(&record).unwrap_err(),
        UNINITIALIZED_PERSONA_ENV_SNAPSHOT_ERROR
    );
    assert_eq!(
        resolve_effective_harness_descriptor(&record, &[], &Default::default()).unwrap_err(),
        UNINITIALIZED_PERSONA_ENV_SNAPSHOT_ERROR
    );
    let effective = resolve_effective_agent_env(
        &record,
        &[],
        known_acp_runtime_exact("buzz-agent"),
        &Default::default(),
    );
    assert_eq!(
        agent_readiness(&effective),
        AgentReadiness::NotReady {
            requirements: vec![Requirement::PersonaSnapshotUninitialized]
        }
    );
}

#[test]
fn intentional_empty_and_standalone_env_states_are_initialized() {
    ensure_persona_env_snapshot_initialized(&record())
        .expect("standalone records need no persona env pin");

    let mut linked = record();
    linked.persona_id = Some("analytics".to_string());
    linked.pinned_persona_env_vars = Some(BTreeMap::new());
    ensure_persona_env_snapshot_initialized(&linked).expect("Some(empty) is intentional");
    resolve_effective_harness_descriptor(&linked, &[], &Default::default())
        .expect("initialized linked record must pass the env gate");
}

#[test]
fn uninitialized_requirement_has_stable_surface() {
    assert_eq!(
        serde_json::to_value(Requirement::PersonaSnapshotUninitialized).unwrap(),
        serde_json::json!({ "surface": "persona_snapshot_uninitialized" })
    );
}
