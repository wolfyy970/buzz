use super::*;
use crate::managed_agents::env_vars::merged_user_env;
use std::collections::BTreeMap;

fn persona_with_env(prompt: &str, value: &str) -> crate::managed_agents::AgentDefinition {
    let mut persona = persona_with_provider("p", prompt, Some("model-v"), Some("anthropic"));
    persona.env_vars = BTreeMap::from([("ANTHROPIC_API_KEY".to_string(), value.to_string())]);
    persona
}

fn pin(record: &mut ManagedAgentRecord, persona: &crate::managed_agents::AgentDefinition) {
    record.persona_id = Some(persona.id.clone());
    crate::managed_agents::persona_events::apply_persona_snapshot(record, persona).unwrap();
}

fn spawn_user_env(record: &ManagedAgentRecord) -> BTreeMap<String, String> {
    merged_user_env(
        record
            .pinned_persona_env_vars
            .as_ref()
            .unwrap_or(&BTreeMap::new()),
        &record.env_vars,
    )
}

#[test]
fn create_pins_persona_env_below_instance_overrides() {
    let persona = persona_with_env("prompt", "persona-key");
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    record.env_vars = BTreeMap::from([("ANTHROPIC_API_KEY".to_string(), "agent-key".to_string())]);
    pin(&mut record, &persona);

    assert_eq!(
        record.pinned_persona_env_vars,
        Some(persona.env_vars.clone())
    );
    assert_eq!(
        spawn_user_env(&record)
            .get("ANTHROPIC_API_KEY")
            .map(String::as_str),
        Some("agent-key")
    );
    assert!(record.persona_source_version.is_some());
}

#[test]
fn restart_preserves_selected_credential_until_explicit_advance() {
    let first = persona_with_env("prompt-v0", "key-v0");
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    pin(&mut record, &first);
    let second = persona_with_env("prompt-v1", "key-v1");

    assert_eq!(
        spawn_user_env(&record)
            .get("ANTHROPIC_API_KEY")
            .map(String::as_str),
        Some("key-v0")
    );
    assert_eq!(
        super::super::persona_drift_state(&record, std::slice::from_ref(&second)),
        (true, false)
    );

    crate::managed_agents::persona_events::advance_persona_snapshot(&mut record, &second).unwrap();
    assert_eq!(
        spawn_user_env(&record)
            .get("ANTHROPIC_API_KEY")
            .map(String::as_str),
        Some("key-v1")
    );
}

#[test]
fn env_only_edit_drifts_until_advanced() {
    let first = persona_with_env("prompt", "key-v0");
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    pin(&mut record, &first);
    let mut second = first.clone();
    second
        .env_vars
        .insert("ANTHROPIC_API_KEY".to_string(), "key-v1".to_string());
    second.updated_at = "2026-06-09T00:00:00.000000001Z".to_string();

    assert_eq!(
        crate::managed_agents::persona_events::persona_content_hash(
            &crate::managed_agents::persona_events::persona_event_content(&first),
        ),
        crate::managed_agents::persona_events::persona_content_hash(
            &crate::managed_agents::persona_events::persona_event_content(&second),
        )
    );
    assert_eq!(
        super::super::persona_drift_state(&record, std::slice::from_ref(&second)),
        (true, false)
    );
    crate::managed_agents::persona_events::advance_persona_snapshot(&mut record, &second).unwrap();
    assert_eq!(
        super::super::persona_drift_state(&record, std::slice::from_ref(&second)),
        (false, false)
    );
}

#[test]
fn mutable_template_edits_do_not_change_a_published_version_pin() {
    let mut published = persona_with_env("published prompt", "published key");
    let version = crate::managed_agents::AgentTemplateVersionRef {
        repo_address: format!("30617:{}:buzz-agent-templates", "a".repeat(64)),
        commit_oid: "b".repeat(40),
        artifact_path: format!("templates/p/versions/{}/template.json", "c".repeat(64)),
        artifact_sha256: "c".repeat(64),
    };
    published.published_version = Some(version.clone());

    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    pin(&mut record, &published);
    record.persona_source_version = Some(version.authority_token().unwrap());

    let mut edited_head = published.clone();
    edited_head.system_prompt = "unpublished edit".to_string();
    edited_head.updated_at = "2026-06-09T00:00:00.000000001Z".to_string();
    assert_eq!(
        super::super::persona_drift_state(&record, std::slice::from_ref(&edited_head)),
        (false, false)
    );

    edited_head.published_version = Some(crate::managed_agents::AgentTemplateVersionRef {
        commit_oid: "d".repeat(40),
        ..version
    });
    assert_eq!(
        super::super::persona_drift_state(&record, std::slice::from_ref(&edited_head)),
        (true, false)
    );
}

#[test]
fn legacy_equal_pseudo_override_is_removed_without_losing_real_override() {
    let first = persona_with_env("prompt", "key-v0");
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    record.env_vars = BTreeMap::from([
        ("ANTHROPIC_API_KEY".to_string(), "key-v0".to_string()),
        ("GENUINE".to_string(), "override".to_string()),
    ]);
    pin(&mut record, &first);

    assert!(!record.env_vars.contains_key("ANTHROPIC_API_KEY"));
    assert_eq!(
        record.env_vars.get("GENUINE").map(String::as_str),
        Some("override")
    );
}

#[test]
fn orphan_and_nonpersona_drift_states_are_distinct() {
    let persona = persona_with_env("prompt", "key");
    let mut linked = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    pin(&mut linked, &persona);
    assert_eq!(
        super::super::persona_drift_state(&linked, &[]),
        (false, true)
    );

    let standalone = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    assert_eq!(
        super::super::persona_drift_state(&standalone, &[]),
        (false, false)
    );
}

#[test]
fn orphaned_linked_agent_is_refused_at_effective_config_boundary() {
    let persona = persona_with_env("prompt", "key");
    let mut record = fixture(RespondTo::Anyone, vec![], Some("tag".into()));
    pin(&mut record, &persona);

    let error = crate::managed_agents::effective_config::resolve_effective_config(
        &record,
        &[],
        &Default::default(),
    )
    .require_resolved()
    .unwrap_err();
    assert_eq!(
        error,
        crate::managed_agents::effective_config::ORPHANED_INSTANCE_ERROR
    );
}
