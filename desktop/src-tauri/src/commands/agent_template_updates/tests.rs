use super::*;

fn persona(updated_at: &str) -> crate::managed_agents::AgentDefinition {
    crate::managed_agents::AgentDefinition {
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
        created_at: "2026-08-02T10:00:00.000000000Z".to_string(),
        updated_at: updated_at.to_string(),
        tool_requirements: Vec::new(),
        skills: Vec::new(),
        published_version: None,
        published_version_env_vars: None,
    }
}

fn version(commit: char) -> AgentTemplateVersionRef {
    AgentTemplateVersionRef {
        repo_address: format!("30617:{}:buzz-agent-templates", "a".repeat(64)),
        commit_oid: commit.to_string().repeat(40),
        artifact_path: format!(
            "templates/analytics/versions/{}/template.json",
            "c".repeat(64)
        ),
        artifact_sha256: "c".repeat(64),
    }
}

fn record() -> ManagedAgentRecord {
    let mut record = persona("2026-08-02T10:00:00.000000001Z").into_agent_record();
    record.pubkey = "a".repeat(64);
    record.persona_id = Some("analytics".to_string());
    record.pinned_persona_env_vars = Some(BTreeMap::new());
    record
}

fn tool(id: &str, label: &str, capability: &str) -> crate::managed_agents::AgentToolRequirement {
    crate::managed_agents::AgentToolRequirement {
        id: id.to_string(),
        label: label.to_string(),
        capability: capability.to_string(),
        required: true,
    }
}

fn skill(name: &str, body: &str) -> crate::managed_agents::AgentSkill {
    let description = format!("{name} description");
    crate::managed_agents::AgentSkill {
        name: name.to_string(),
        description: description.clone(),
        files: vec![crate::managed_agents::AgentSkillFile {
            path: "SKILL.md".to_string(),
            content: format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
        }],
    }
}

#[test]
fn selection_rejects_duplicates() {
    let selected = vec!["aa".repeat(32), "AA".repeat(32)];
    let error = validate_selection(&[], "analytics", &selected)
        .expect_err("duplicate selection should fail before lookup");
    assert!(error.contains("selected more than once"));
}

#[test]
fn expected_version_check_rejects_a_newer_published_version() {
    let mut previewed = persona("2026-08-02T10:00:00.000000001Z");
    previewed.published_version = Some(version('b'));
    let mut edited = previewed.clone();
    edited.published_version = Some(version('d'));
    let expected = previewed.published_version.clone().unwrap();

    assert_eq!(
        checked_published_version(&previewed, &expected).unwrap(),
        expected
    );
    assert!(
        checked_published_version(&edited, &expected)
            .unwrap_err()
            .contains("newer template version"),
        "the published version reloaded under the store lock must be revalidated"
    );
}

#[test]
fn tool_changes_are_computed_per_agent_snapshot() {
    let before = vec![
        tool("analytics", "Analytics", "mcp.tool.old_report"),
        tool("removed", "Legacy export", "mcp.tool.legacy"),
    ];
    let after = vec![
        tool("analytics", "Analytics", "mcp.tool.run_report"),
        tool("issues", "Issue tracker", "mcp.tool.create_issue"),
    ];

    let changes = tool_changes(&before, &after);

    assert_eq!(changes.added, vec![after[1].clone()]);
    assert_eq!(changes.changed.len(), 1);
    assert_eq!(changes.changed[0].before, before[0]);
    assert_eq!(changes.changed[0].after, after[0]);
    assert_eq!(changes.removed, vec![before[1].clone()]);
}

#[test]
fn skill_changes_are_computed_per_agent_snapshot() {
    let before = vec![
        skill("analysis", "Old workflow"),
        skill("legacy-export", "Legacy workflow"),
    ];
    let after = vec![
        skill("analysis", "New workflow"),
        skill("issue-tracker", "Create issues"),
    ];

    let changes = skill_changes(&before, &after);

    assert_eq!(changes.added, vec![after[1].clone()]);
    assert_eq!(changes.changed.len(), 1);
    assert_eq!(changes.changed[0].before, before[0]);
    assert_eq!(changes.changed[0].after, after[0]);
    assert_eq!(changes.removed, vec![before[1].clone()]);
}

#[test]
fn advancing_a_template_pin_preserves_agent_only_overrides() {
    let mut original = record();
    original.system_prompt_override = Some("Only this agent".to_string());
    original.skill_overrides = Some(vec![skill("private-analysis", "Private workflow")]);
    let mut target = persona("2026-08-02T10:00:00.000000002Z");
    target.system_prompt = "New template instructions".to_string();
    target.skills = vec![skill("template-analysis", "Template workflow")];

    let prospective = prospective_record(&original, &target, BTreeMap::new()).unwrap();

    assert_eq!(
        prospective.system_prompt.as_deref(),
        Some("New template instructions")
    );
    assert_eq!(prospective.pinned_skills, target.skills);
    assert_eq!(
        prospective.system_prompt_override,
        original.system_prompt_override
    );
    assert_eq!(prospective.skill_overrides, original.skill_overrides);
}

#[test]
fn preview_eligibility_blocks_remote_and_unready_local_agents() {
    let readiness_error = "Analytics needs an API key.".to_string();
    assert_eq!(
        template_update_blocked_reason(&BackendKind::Local, None),
        None
    );
    assert_eq!(
        template_update_blocked_reason(&BackendKind::Local, Some(readiness_error.clone())),
        Some(readiness_error)
    );
    assert_eq!(
        template_update_blocked_reason(
            &BackendKind::Provider {
                id: "cloud".to_string(),
                config: serde_json::Value::Null,
            },
            Some("ignored".to_string()),
        ),
        Some("Remote agents cannot be updated safely in this version.".to_string())
    );
}

#[test]
fn readiness_formatter_is_actionable_without_rust_debug_syntax() {
    let text = format_readiness_requirements(&[
        crate::managed_agents::Requirement::NormalizedField {
            field: "provider".to_string(),
        },
        crate::managed_agents::Requirement::EnvKey {
            key: "OPENAI_COMPAT_API_KEY".to_string(),
        },
        crate::managed_agents::Requirement::MissingBinary {
            command: "pi".to_string(),
        },
    ]);
    assert_eq!(
        text,
        "choose a provider; set the required OPENAI_COMPAT_API_KEY environment variable; install pi or add it to PATH"
    );
    assert!(!text.contains("Requirement"));
    assert!(!text.contains('{'));
}

#[test]
fn rollback_guard_rejects_concurrent_config_with_same_persona_version() {
    let original = record();
    let mut attempted = original.clone();
    attempted.system_prompt = Some("new prompt".to_string());
    attempted.persona_source_version = Some("new-version".to_string());
    let mut concurrent = attempted.clone();
    concurrent
        .env_vars
        .insert("INSTANCE_ONLY".to_string(), "new-value".to_string());

    assert!(rollback_guard_allows(&attempted, &original, &attempted));
    assert!(!rollback_guard_allows(&concurrent, &original, &attempted));
}

#[test]
fn rollback_guard_accepts_atomic_save_original_and_preserves_runtime_state() {
    let original = record();
    let mut attempted = original.clone();
    attempted.system_prompt = Some("new prompt".to_string());
    assert!(rollback_guard_allows(&original, &original, &attempted));

    attempted.runtime_pid = Some(42);
    attempted.updated_at = "runtime-update".to_string();
    attempted.last_started_at = Some("started".to_string());
    assert!(same_nonvolatile_config(&attempted, &{
        let mut expected = attempted.clone();
        expected.runtime_pid = None;
        expected.updated_at.clear();
        expected.last_started_at = None;
        expected
    }));

    restore_config_preserving_runtime_state(&mut attempted, &original);
    assert_eq!(attempted.runtime_pid, Some(42));
    assert_eq!(attempted.updated_at, "runtime-update");
    assert_eq!(attempted.last_started_at.as_deref(), Some("started"));
    assert!(same_nonvolatile_config(&attempted, &original));
}
