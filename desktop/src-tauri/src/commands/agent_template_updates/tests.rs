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
    let mut changed_tool = tool("analytics", "Campaign reports", "mcp.tool.run_report");
    changed_tool.required = false;
    let after = vec![
        changed_tool,
        tool("issues", "Issue tracker", "mcp.tool.create_issue"),
    ];

    let changes = tool_changes(&before, &after);

    assert_eq!(changes.added, vec![after[1].clone()]);
    assert_eq!(changes.changed.len(), 1);
    assert_eq!(changes.changed[0].before, before[0]);
    assert_eq!(changes.changed[0].after, after[0]);
    assert!(changes.changed[0].label_changed);
    assert!(changes.changed[0].capability_changed);
    assert!(changes.changed[0].required_changed);
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
fn changed_skills_disclose_exact_file_content_changes() {
    let before = crate::managed_agents::AgentSkill {
        name: "analysis".to_string(),
        description: "Analyze reports".to_string(),
        files: vec![
            crate::managed_agents::AgentSkillFile {
                path: "SKILL.md".to_string(),
                content: "old instructions".to_string(),
            },
            crate::managed_agents::AgentSkillFile {
                path: "references/legacy.md".to_string(),
                content: "legacy reference".to_string(),
            },
        ],
    };
    let after = crate::managed_agents::AgentSkill {
        name: "analysis".to_string(),
        description: "Analyze reports carefully".to_string(),
        files: vec![
            crate::managed_agents::AgentSkillFile {
                path: "SKILL.md".to_string(),
                content: "new instructions".to_string(),
            },
            crate::managed_agents::AgentSkillFile {
                path: "references/current.md".to_string(),
                content: "current reference".to_string(),
            },
        ],
    };

    let changes = skill_changes(std::slice::from_ref(&before), std::slice::from_ref(&after));
    let file_changes = &changes.changed[0].file_changes;

    assert_eq!(
        file_changes.changed,
        vec![AgentTemplateSkillFileContentChange {
            path: "SKILL.md".to_string(),
            before: "old instructions".to_string(),
            after: "new instructions".to_string(),
        }]
    );
    assert_eq!(
        file_changes.added,
        vec![AgentTemplateSkillFileContent {
            path: "references/current.md".to_string(),
            content: "current reference".to_string(),
        }]
    );
    assert_eq!(
        file_changes.removed,
        vec![AgentTemplateSkillFileContent {
            path: "references/legacy.md".to_string(),
            content: "legacy reference".to_string(),
        }]
    );
}

#[test]
fn instruction_changes_are_exact_and_mark_private_overrides() {
    let mut current = record();
    current.system_prompt =
        Some("Current instructions.\n||Show this syntax literally.||".to_string());
    current.system_prompt_override = Some("Only this agent.".to_string());
    let mut target = persona("2026-08-02T10:00:00.000000002Z");
    target.system_prompt =
        "New instructions.\n[Do not project this](https://example.test)".to_string();

    let change = instruction_change(&current, &target).expect("instructions changed");

    assert_eq!(
        change.before,
        "Current instructions.\n||Show this syntax literally.||"
    );
    assert_eq!(
        change.after,
        "New instructions.\n[Do not project this](https://example.test)"
    );
    assert!(change.private_override_preserved);
    assert!(!serde_json::to_string(&change)
        .expect("serialize instruction change")
        .contains("Only this agent"));
}

#[test]
fn unchanged_instructions_have_no_diff() {
    let mut current = record();
    current.system_prompt = Some("Analyze the data.".to_string());

    assert_eq!(instruction_change(&current, &persona("updated")), None);
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
fn preview_changes_disclose_effective_settings_and_only_relevant_override_keys() {
    let mut current = record();
    current.runtime = Some("codex".to_string());
    current.provider = Some("anthropic".to_string());
    current.model = Some("old-model".to_string());
    current.respond_to = crate::managed_agents::RespondTo::OwnerOnly;
    let alice_pubkey = "11".repeat(32);
    let bob_pubkey = "22".repeat(32);
    let shared_pubkey = "33".repeat(32);
    current.respond_to_allowlist = vec![alice_pubkey.clone(), shared_pubkey.clone()];
    current.parallelism = 1;
    current.pinned_persona_env_vars = Some(BTreeMap::from([
        ("API_KEY".to_string(), "old-template-secret".to_string()),
        ("REMOVED_KEY".to_string(), "removed-secret".to_string()),
        ("UNCHANGED".to_string(), "same".to_string()),
    ]));
    current.env_vars = BTreeMap::from([
        ("API_KEY".to_string(), "agent-secret".to_string()),
        (
            "UNRELATED_PRIVATE_KEY".to_string(),
            "unrelated-secret".to_string(),
        ),
    ]);
    current.system_prompt_override = Some("private instructions".to_string());
    current.agent_command_override = Some("private-harness".to_string());
    current.model_override = Some("private-model".to_string());
    current.provider_override = Some("private-provider".to_string());
    current.pinned_skills = vec![skill("analysis", "Old template workflow")];
    current.skill_overrides = Some(vec![skill("private-analysis", "Private workflow")]);

    let mut target = persona("2026-08-02T10:00:00.000000002Z");
    target.display_name = "Identity must not be an instance impact".to_string();
    target.name_pool = vec!["Future-only name".to_string()];
    target.runtime = Some("pi".to_string());
    target.provider = Some("openai".to_string());
    target.model = Some("new-model".to_string());
    target.system_prompt = "New template instructions".to_string();
    target.respond_to = Some("allowlist".to_string());
    target.respond_to_allowlist = vec![bob_pubkey.clone(), shared_pubkey];
    target.parallelism = Some(4);
    target.env_vars = BTreeMap::from([
        ("ADDED_KEY".to_string(), "added-secret".to_string()),
        ("API_KEY".to_string(), "new-template-secret".to_string()),
        ("UNCHANGED".to_string(), "same".to_string()),
    ]);
    target.skills = vec![skill("analysis", "New template workflow")];

    let target_snapshot = crate::managed_agents::persona_events::persona_snapshot(&target).unwrap();
    let instruction_change = instruction_change(&current, &target);
    let skill_changes = skill_changes(&current.pinned_skills, &target.skills);
    let changes = version_changes(&current, &target_snapshot);
    let preserved = preserved_overrides(
        &current,
        instruction_change.as_ref(),
        &skill_changes,
        &changes,
    );

    assert_eq!(
        changes.runtime,
        Some(AgentTemplateOptionalStringChange {
            before: Some("codex".to_string()),
            after: Some("pi".to_string()),
        })
    );
    assert_eq!(
        changes.provider,
        Some(AgentTemplateOptionalStringChange {
            before: Some("anthropic".to_string()),
            after: Some("openai".to_string()),
        })
    );
    assert_eq!(
        changes.model,
        Some(AgentTemplateOptionalStringChange {
            before: Some("old-model".to_string()),
            after: Some("new-model".to_string()),
        })
    );
    assert_eq!(
        changes.access,
        Some(AgentTemplateAccessChange {
            before: crate::managed_agents::RespondTo::OwnerOnly,
            after: crate::managed_agents::RespondTo::Allowlist,
            allowlist_added: vec![bob_pubkey],
            allowlist_removed: vec![alice_pubkey],
        })
    );
    assert_eq!(
        changes.parallelism,
        Some(AgentTemplateParallelismChange {
            before: 1,
            after: 4,
        })
    );
    assert_eq!(
        changes.environment,
        AgentTemplateEnvironmentChanges {
            added_keys: vec!["ADDED_KEY".to_string()],
            changed_keys: vec!["API_KEY".to_string()],
            removed_keys: vec!["REMOVED_KEY".to_string()],
        }
    );
    assert_eq!(
        preserved,
        AgentTemplateOverridesPreserved {
            instructions: true,
            runtime: true,
            model: true,
            provider: true,
            skills: true,
            local_environment: true,
            local_environment_keys: vec!["API_KEY".to_string()],
        }
    );

    let json = serde_json::to_string(&(changes, preserved)).unwrap();
    for secret in [
        "old-template-secret",
        "removed-secret",
        "agent-secret",
        "unrelated-secret",
        "added-secret",
        "new-template-secret",
        "private instructions",
        "private-harness",
        "private-model",
        "private-provider",
        "Private workflow",
        "UNRELATED_PRIVATE_KEY",
        "Identity must not be an instance impact",
        "Future-only name",
    ] {
        assert!(
            !json.contains(secret),
            "preview change metadata leaked {secret:?}"
        );
    }
    assert!(json.contains("API_KEY"));
}

#[test]
fn override_flags_ignore_private_settings_unrelated_to_this_version() {
    let mut current = record();
    current.system_prompt_override = Some("private instructions".to_string());
    current.agent_command_override = Some("private-harness".to_string());
    current.model_override = Some("private-model".to_string());
    current.provider_override = Some("private-provider".to_string());
    current.skill_overrides = Some(vec![skill("private-analysis", "Private workflow")]);
    current.env_vars.insert(
        "UNRELATED_PRIVATE_KEY".to_string(),
        "private value".to_string(),
    );
    let target = persona("unchanged");
    let target_snapshot = crate::managed_agents::persona_events::persona_snapshot(&target).unwrap();
    let instructions = instruction_change(&current, &target);
    let skills = skill_changes(&current.pinned_skills, &target.skills);
    let changes = version_changes(&current, &target_snapshot);

    assert_eq!(
        preserved_overrides(&current, instructions.as_ref(), &skills, &changes),
        AgentTemplateOverridesPreserved::default()
    );
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
fn template_updates_pin_the_current_global_runtime_defaults() {
    let mut target = persona("2026-08-02T10:00:00.000000002Z");
    target.runtime = None;
    let global = crate::managed_agents::GlobalAgentConfig {
        preferred_runtime: Some("codex".to_string()),
        provider: Some("openai".to_string()),
        model: Some("gpt-5".to_string()),
        ..Default::default()
    };
    let mut prospective = record();

    crate::managed_agents::persona_events::apply_persona_snapshot(&mut prospective, &target)
        .unwrap();
    materialize_template_runtime_defaults(&mut prospective, &global);

    assert_eq!(prospective.runtime.as_deref(), Some("codex"));
    assert_eq!(prospective.provider.as_deref(), Some("openai"));
    assert_eq!(prospective.model.as_deref(), Some("gpt-5"));
}

#[test]
fn explicit_template_runtime_values_win_over_global_defaults() {
    let global = crate::managed_agents::GlobalAgentConfig {
        preferred_runtime: Some("codex".to_string()),
        provider: Some("openai".to_string()),
        model: Some("gpt-5".to_string()),
        ..Default::default()
    };
    let mut prospective = record();
    prospective.runtime = Some("claude".to_string());
    prospective.provider = Some("anthropic".to_string());
    prospective.model = Some("claude-opus-4-5".to_string());

    materialize_template_runtime_defaults(&mut prospective, &global);

    assert_eq!(prospective.runtime.as_deref(), Some("claude"));
    assert_eq!(prospective.provider.as_deref(), Some("anthropic"));
    assert_eq!(prospective.model.as_deref(), Some("claude-opus-4-5"));
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

#[test]
fn drain_failure_never_treats_a_missing_uncheckpointed_runtime_as_restartable() {
    let before_acceptance = ManagedAgentUpdateDrainError {
        message: "exited before acceptance".to_string(),
        rollback_safe: true,
    };
    let after_acceptance = ManagedAgentUpdateDrainError {
        message: "ownership uncertain".to_string(),
        rollback_safe: false,
    };

    assert_eq!(
        classify_drain_failure(&before_acceptance, true),
        DrainFailureState::OriginalStillRunning
    );
    assert_eq!(
        classify_drain_failure(&before_acceptance, false),
        DrainFailureState::ExitedWithoutCheckpoint
    );
    assert_eq!(
        classify_drain_failure(&after_acceptance, true),
        DrainFailureState::OwnershipUncertain
    );
    assert_eq!(
        classify_drain_failure(&after_acceptance, false),
        DrainFailureState::OwnershipUncertain
    );
}

#[test]
fn rollback_failure_is_returned_as_an_agent_outcome() {
    let pubkey = "a".repeat(64);
    let response = failure_response(
        "analytics".to_string(),
        version('d'),
        std::slice::from_ref(&pubkey),
        &BTreeMap::from([(pubkey.clone(), "Analyst".to_string())]),
        false,
        AgentTemplateUpdateOutcome::RollbackFailed,
        "ownership could not be proved".to_string(),
    );
    let json = serde_json::to_value(response).expect("serialize response");

    assert_eq!(json["rolledBack"], false);
    assert_eq!(json["agents"][0]["outcome"], "rollback_failed");
    assert_eq!(json["agents"][0]["name"], "Analyst");
    assert_eq!(json["agents"][0]["error"], "ownership could not be proved");
}

#[test]
fn update_progress_requires_a_frontend_request_uuid() {
    assert!(validate_update_request_id("4f62dd32-2f13-42ca-8c1d-c455149a0eef").is_ok());
    assert!(validate_update_request_id("").is_err());
    assert!(validate_update_request_id("shared-request-name").is_err());
}

#[test]
fn update_progress_payload_uses_product_stage_names() {
    let payload = AgentTemplateUpdateProgress {
        request_id: "4f62dd32-2f13-42ca-8c1d-c455149a0eef".to_string(),
        stage: AgentTemplateUpdateProgressStage::FinishingCurrentTask,
    };
    let json = serde_json::to_value(payload).expect("serialize progress");

    assert_eq!(json["requestId"], "4f62dd32-2f13-42ca-8c1d-c455149a0eef");
    assert_eq!(json["stage"], "finishing_current_task");
}
