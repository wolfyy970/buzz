use super::*;

fn exact_legacy_record(persona: &AgentDefinition) -> ManagedAgentRecord {
    let mut record = sample_record();
    let snapshot = persona_snapshot(persona).unwrap();
    record.system_prompt = Some(persona.system_prompt.clone());
    record.model = persona.model.clone();
    record.provider = persona.provider.clone();
    record.runtime = persona.runtime.clone();
    record.respond_to = snapshot.respond_to;
    record.respond_to_allowlist = snapshot.respond_to_allowlist;
    record.parallelism = snapshot.parallelism;
    record.persona_source_version = Some(persona_content_hash(&persona_event_content(persona)));
    record
}

#[test]
fn existing_nonlegacy_version_backfills_only_env_and_is_idempotent() {
    let persona = sample_persona();
    let mut record = sample_record();
    record.system_prompt = Some("selected prompt".into());
    record.model = Some("selected-model".into());
    record.provider = Some("selected-provider".into());
    record.runtime = Some("selected-runtime".into());
    record.persona_source_version = Some("selected-version".into());
    record.env_vars.insert("KEY".into(), "value".into());
    let selected = (
        record.system_prompt.clone(),
        record.model.clone(),
        record.provider.clone(),
        record.runtime.clone(),
        record.persona_source_version.clone(),
    );

    assert!(backfill_persona_snapshot(&mut record, &persona).unwrap());
    assert_eq!(
        (
            record.system_prompt.clone(),
            record.model.clone(),
            record.provider.clone(),
            record.runtime.clone(),
            record.persona_source_version.clone(),
        ),
        selected
    );
    assert_eq!(
        record.pinned_persona_env_vars,
        Some(persona.env_vars.clone())
    );
    assert!(!record.env_vars.contains_key("KEY"));
    assert!(!backfill_persona_snapshot(&mut record, &persona).unwrap());
}

#[test]
fn missing_version_backfill_applies_complete_revision() {
    let persona = sample_persona();
    let mut record = sample_record();

    assert!(backfill_persona_snapshot(&mut record, &persona).unwrap());
    assert_eq!(
        record.system_prompt.as_deref(),
        Some(persona.system_prompt.as_str())
    );
    assert_eq!(record.model, persona.model);
    assert_eq!(record.provider, persona.provider);
    assert_eq!(record.runtime, persona.runtime);
    assert_eq!(
        record.pinned_persona_env_vars,
        Some(persona.env_vars.clone())
    );
    assert_eq!(
        record.persona_source_version,
        Some(persona_snapshot_version(&persona))
    );
}

#[test]
fn exact_legacy_snapshot_migrates_to_revision_token() {
    let persona = sample_persona();
    let mut record = exact_legacy_record(&persona);

    assert!(backfill_persona_snapshot(&mut record, &persona).unwrap());
    assert_eq!(
        record.pinned_persona_env_vars,
        Some(persona.env_vars.clone())
    );
    assert_eq!(
        record.persona_source_version,
        Some(persona_snapshot_version(&persona))
    );
    assert!(!backfill_persona_snapshot(&mut record, &persona).unwrap());
}

#[test]
fn legacy_snapshot_with_mismatched_prompt_stays_stale() {
    let persona = sample_persona();
    let legacy = persona_content_hash(&persona_event_content(&persona));
    let mut record = exact_legacy_record(&persona);
    record.system_prompt = Some("older selected prompt".to_string());

    assert!(backfill_persona_snapshot(&mut record, &persona).unwrap());
    assert_eq!(
        record.pinned_persona_env_vars,
        Some(persona.env_vars.clone())
    );
    assert_eq!(record.persona_source_version, Some(legacy));
}

#[test]
fn initialized_legacy_env_migrates_only_when_it_matches_head() {
    let persona = sample_persona();
    let legacy = persona_content_hash(&persona_event_content(&persona));
    let mut exact = exact_legacy_record(&persona);
    exact.pinned_persona_env_vars = Some(persona.env_vars.clone());
    assert!(backfill_persona_snapshot(&mut exact, &persona).unwrap());
    assert_eq!(
        exact.persona_source_version,
        Some(persona_snapshot_version(&persona))
    );

    let mut stale = exact_legacy_record(&persona);
    stale.pinned_persona_env_vars = Some(BTreeMap::from([(
        "KEY".to_string(),
        "older-value".to_string(),
    )]));
    assert!(!backfill_persona_snapshot(&mut stale, &persona).unwrap());
    assert_eq!(stale.persona_source_version, Some(legacy));
}

#[test]
fn env_only_save_changes_revision_without_hashing_env_values() {
    let original = sample_persona();
    let mut edited = original.clone();
    edited
        .env_vars
        .insert("KEY".to_string(), "rotated-secret-value".to_string());

    assert_eq!(
        persona_content_hash(&persona_event_content(&original)),
        persona_content_hash(&persona_event_content(&edited))
    );
    assert_eq!(
        persona_snapshot_version(&original),
        persona_snapshot_version(&edited),
        "secret values must not contribute to the revision token"
    );

    edited.updated_at = "2025-01-01T00:00:00.000000001Z".to_string();
    let edited_version = persona_snapshot_version(&edited);
    assert_ne!(persona_snapshot_version(&original), edited_version);
    assert!(!edited_version.contains("rotated-secret-value"));
}
