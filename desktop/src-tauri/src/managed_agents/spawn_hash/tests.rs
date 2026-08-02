use super::*;
use crate::managed_agents::types::RespondTo;
use std::collections::BTreeMap;

fn record() -> ManagedAgentRecord {
    ManagedAgentRecord {
        pubkey: "p".repeat(64),
        name: "agent".into(),
        persona_id: None,
        private_key_nsec: "nsec1fake".into(),
        auth_tag: None,
        relay_url: "ws://localhost:3000".into(),
        avatar_url: None,
        acp_command: "buzz-acp".into(),
        agent_command: "goose".into(),
        agent_command_override: None,
        agent_args: vec![],
        mcp_command: String::new(),
        turn_timeout_seconds: 320,
        idle_timeout_seconds: None,
        max_turn_duration_seconds: None,
        parallelism: 1,
        system_prompt: Some("You are a test agent.".into()),
        model: None,
        provider: None,
        persona_source_version: None,
        pinned_persona_env_vars: Default::default(),
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
        created_at: "now".into(),
        updated_at: "now".into(),
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
        system_prompt_override: None,
        model_override: None,
        provider_override: None,
        pinned_skills: Vec::new(),
        skill_overrides: None,
    }
}

fn persona(id: &str, runtime: Option<&str>, prompt: &str) -> AgentDefinition {
    AgentDefinition {
        published_version: None,
        published_version_env_vars: None,
        id: id.into(),
        display_name: id.into(),
        avatar_url: None,
        system_prompt: prompt.into(),
        runtime: runtime.map(str::to_string),
        model: None,
        provider: None,
        name_pool: vec![],
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
        created_at: "now".into(),
        updated_at: "now".into(),
        tool_requirements: Vec::new(),
        skills: Vec::new(),
    }
}

fn skill(name: &str, body: &str) -> crate::managed_agents::AgentSkill {
    let description = "A test skill";
    crate::managed_agents::AgentSkill {
        name: name.to_string(),
        description: description.to_string(),
        files: vec![crate::managed_agents::AgentSkillFile {
            path: "SKILL.md".to_string(),
            content: format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
        }],
    }
}

#[test]
fn hash_is_deterministic() {
    let rec = record();
    assert_eq!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn mutable_definition_runtime_does_not_change_pinned_hash() {
    let mut rec = record();
    rec.persona_id = Some("p1".into());
    rec.runtime = Some("goose".into());
    let before = vec![persona("p1", Some("goose"), "Persona prompt.")];
    let after = vec![persona("p1", Some("claude"), "Persona prompt.")];
    assert_eq!(
        spawn_config_hash(&rec, &before, &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &after, &[], "wss://ws.example", &Default::default()),
        "editing the definition head must not alter a selected instance"
    );
}

#[test]
fn record_env_var_edit_changes_hash() {
    let rec = record();
    let mut edited = record();
    edited
        .env_vars
        .insert("SOME_KEY".into(), "some-value".into());
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn pinned_persona_env_edit_changes_hash() {
    let mut rec = record();
    rec.pinned_persona_env_vars = Some(BTreeMap::from([("PERSONA_KEY".into(), "v1".into())]));
    let mut edited = rec.clone();
    edited.pinned_persona_env_vars = Some(BTreeMap::from([("PERSONA_KEY".into(), "v2".into())]));
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn mutable_definition_env_edit_does_not_change_pinned_hash() {
    let mut rec = record();
    rec.persona_id = Some("pers".into());
    rec.pinned_persona_env_vars = Some(BTreeMap::from([("PERSONA_KEY".into(), "selected".into())]));
    let before = persona("pers", Some("goose"), "prompt");
    let mut after = before.clone();
    after
        .env_vars
        .insert("PERSONA_KEY".into(), "new-head".into());
    assert_eq!(
        spawn_config_hash(
            &rec,
            &[before],
            &[],
            "wss://ws.example",
            &Default::default()
        ),
        spawn_config_hash(&rec, &[after], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn record_prompt_edit_changes_hash() {
    let rec = record();
    let mut edited = record();
    edited.system_prompt = Some("Edited prompt.".into());
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn private_skill_edit_changes_hash_without_mutating_the_pin() {
    let mut original = record();
    original.pinned_skills = vec![skill("analysis", "Template workflow")];
    let mut edited = original.clone();
    edited.skill_overrides = Some(vec![skill("analysis", "Agent-only workflow")]);

    assert_ne!(
        spawn_config_hash(&original, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
    assert_eq!(original.pinned_skills, edited.pinned_skills);
}

#[test]
fn persona_runtime_edit_does_not_change_hash() {
    let mut rec = record();
    rec.persona_id = Some("pers".into());
    rec.runtime = Some("goose".into());
    let before = [persona("pers", Some("goose"), "prompt")];
    let after = [persona("pers", Some("claude"), "prompt")];
    assert_eq!(
        spawn_config_hash(&rec, &before, &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &after, &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn persona_prompt_edit_does_not_change_hash() {
    let mut rec = record();
    rec.persona_id = Some("pers".into());
    let before = [persona("pers", Some("goose"), "old prompt")];
    let after = [persona("pers", Some("goose"), "new prompt")];
    assert_eq!(
        spawn_config_hash(&rec, &before, &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &after, &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn workspace_relay_change_trips_hash_even_for_stored_record_relay() {
    // The legacy per-record relay pin is ignored (#2122): every record spawns
    // against the active workspace relay, so a workspace relay change means a
    // restart would change what runs — pinned records included.
    let rec = record();
    assert!(
        !rec.relay_url.is_empty(),
        "fixture should carry a legacy pin"
    );
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://relay-a.example", &Default::default()),
        spawn_config_hash(&rec, &[], &[], "wss://relay-b.example", &Default::default())
    );
}

#[test]
fn stored_record_relay_does_not_affect_hash() {
    // Editing the (ignored) stored pin must not badge a restart: what a
    // restart would run is identical either way.
    let mut a = record();
    let mut b = record();
    a.relay_url = String::new();
    b.relay_url = "wss://legacy-pin.example".into();
    assert_eq!(
        spawn_config_hash(&a, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&b, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn respond_to_allowlist_edit_changes_hash() {
    let rec = record();
    let mut edited = record();
    edited.respond_to = RespondTo::Allowlist;
    edited.respond_to_allowlist = vec!["a".repeat(64)];
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn allowlist_ignored_when_mode_is_not_allowlist() {
    // Spawn only sets BUZZ_ACP_RESPOND_TO_ALLOWLIST in allowlist mode, so
    // editing the (dormant) list under owner-only must not badge.
    let rec = record();
    let mut edited = record();
    edited.respond_to_allowlist = vec!["a".repeat(64)];
    assert_eq!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn allowlist_normalization_equivalent_edits_do_not_change_hash() {
    // The env receives the normalized list (trim/lowercase/dedup), so edits
    // that normalize to the same value must not badge.
    let mut rec = record();
    rec.respond_to = RespondTo::Allowlist;
    rec.respond_to_allowlist = vec!["a".repeat(64)];
    let mut edited = rec.clone();
    edited.respond_to_allowlist = vec![
        format!(" {} ", "A".repeat(64)), // whitespace + case
        "a".repeat(64),                  // duplicate
    ];
    assert_eq!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn allowlist_content_edit_still_changes_hash() {
    let mut rec = record();
    rec.respond_to = RespondTo::Allowlist;
    rec.respond_to_allowlist = vec!["a".repeat(64)];
    let mut edited = rec.clone();
    edited.respond_to_allowlist = vec!["b".repeat(64)];
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn explicit_max_turn_duration_changes_hash_from_none() {
    let rec = record();
    let mut edited = record();
    edited.max_turn_duration_seconds = Some(7200);
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn non_default_max_turn_duration_changes_hash() {
    let rec = record();
    let mut edited = record();
    edited.max_turn_duration_seconds = Some(42);
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn non_spawn_bookkeeping_fields_do_not_change_hash() {
    // updated_at / runtime_pid / last_* are lifecycle bookkeeping, not spawn
    // inputs — routine record saves must not trip the badge.
    let rec = record();
    let mut edited = record();
    edited.updated_at = "later".into();
    edited.runtime_pid = Some(12345);
    edited.last_started_at = Some("later".into());
    edited.last_exit_code = Some(0);
    assert_eq!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default())
    );
}

#[test]
fn mutable_definition_quad_does_not_clobber_record_quad() {
    // An existing instance's owner-selected behavioral fields are independent
    // of later definition-head edits.
    let quadless_definition = vec![persona("p1", Some("goose"), "Persona prompt.")];

    let mut rec = record();
    rec.persona_id = Some("p1".into());
    rec.respond_to = RespondTo::Allowlist;
    rec.respond_to_allowlist = vec!["a".repeat(64)];
    rec.parallelism = 4;

    let mut definition_with_quad = quadless_definition.clone();
    definition_with_quad[0].respond_to = Some("anyone".into());
    definition_with_quad[0].parallelism = Some(8);

    assert_eq!(
        spawn_config_hash(
            &rec,
            &quadless_definition,
            &[],
            "wss://ws.example",
            &Default::default()
        ),
        spawn_config_hash(
            &rec,
            &definition_with_quad,
            &[],
            "wss://ws.example",
            &Default::default()
        ),
        "definition quad must not leak into the spawn hash of an existing instance"
    );
}

#[test]
fn empty_prompt_hashes_like_absent_prompt() {
    // B5 hash row 2 foundation: Some("") and None spawn identically (env var
    // absent either way), so they must hash equal.
    let mut absent = record();
    absent.system_prompt = None;
    let mut empty = record();
    empty.system_prompt = Some(String::new());
    assert_eq!(
        spawn_config_hash(&absent, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&empty, &[], &[], "wss://ws.example", &Default::default()),
    );
}

/// A mutable definition-runtime edit is inert for a materialized,
/// override-free record.
#[test]
fn definition_runtime_edit_is_inert_for_materialized_record() {
    let mut rec = record();
    rec.persona_id = Some("pers".into());
    rec.runtime = Some("goose".into()); // materialized runtime on instance

    let before = [persona("pers", Some("goose"), "prompt")];
    let after = [persona("pers", Some("claude"), "prompt")];
    assert_eq!(
        spawn_config_hash(&rec, &before, &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &after, &[], "wss://ws.example", &Default::default()),
        "definition runtime edit must not advance a selected instance"
    );
}

/// An explicit known-runtime override remains authoritative across mutable
/// definition edits.
#[test]
fn known_runtime_pin_survives_definition_runtime_change() {
    let mut rec = record();
    rec.persona_id = Some("pers".into());
    rec.runtime = Some("goose".into()); // materialized runtime
    rec.agent_command_override = Some("goose".into()); // create-time pin

    let before = [persona("pers", Some("goose"), "prompt")];
    let after = [persona("pers", Some("claude"), "prompt")];
    assert_eq!(
        spawn_config_hash(&rec, &before, &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &after, &[], "wss://ws.example", &Default::default()),
        "explicit known-runtime override must remain pinned"
    );
}

/// (c2) A custom-command override (no matching known runtime) still beats a
/// changed definition runtime — the badge must NOT fire for such a pin.
#[test]
fn custom_command_override_beats_definition_runtime_change() {
    let mut rec = record();
    rec.persona_id = Some("pers".into());
    rec.runtime = Some("goose".into()); // materialized runtime
    rec.agent_command_override = Some("/opt/custom/my-agent".into());

    let before = [persona("pers", Some("goose"), "prompt")];
    let after = [persona("pers", Some("claude"), "prompt")];
    assert_eq!(
        spawn_config_hash(&rec, &before, &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &after, &[], "wss://ws.example", &Default::default()),
        "custom command override must win regardless of definition runtime change"
    );
}

/// When the linked definition is absent, the materialized runtime still
/// affects the display hash even though actual spawn fails closed.
#[test]
fn missing_definition_leaves_materialized_runtime_in_hash() {
    let mut rec = record();
    rec.persona_id = Some("missing".into());
    rec.runtime = Some("goose".into()); // materialized runtime

    let no_personas: &[AgentDefinition] = &[];

    let mut no_runtime = rec.clone();
    no_runtime.runtime = None;

    assert_ne!(
        spawn_config_hash(
            &rec,
            no_personas,
            &[],
            "wss://ws.example",
            &Default::default()
        ),
        spawn_config_hash(
            &no_runtime,
            no_personas,
            &[],
            "wss://ws.example",
            &Default::default()
        ),
        "materialized runtime must still affect hash when definition is absent"
    );
}

// ── Global default trips hash for linked inherited agents ─────────────────

#[test]
fn global_model_change_trips_hash_for_linked_inherited_agent() {
    let mut rec = record();
    rec.persona_id = Some("p1".into());
    rec.model = None;

    let personas = vec![persona("p1", Some("goose"), "prompt")];

    let global_a = GlobalAgentConfig {
        model: Some("model-a".to_string()),
        provider: Some("prov-a".to_string()),
        ..Default::default()
    };
    let global_b = GlobalAgentConfig {
        model: Some("model-b".to_string()),
        provider: Some("prov-b".to_string()),
        ..Default::default()
    };

    let hash_a = spawn_config_hash(&rec, &personas, &[], "wss://ws.example", &global_a);
    let hash_b = spawn_config_hash(&rec, &personas, &[], "wss://ws.example", &global_b);

    assert_ne!(
        hash_a, hash_b,
        "changing the global default must trip the hash for a linked inherited agent"
    );
}

#[test]
fn global_model_change_trips_hash_without_model_env_var() {
    let mut rec = record();
    rec.persona_id = Some("p1".into());
    rec.agent_command = "some-harness-without-model-env".into();

    let personas = vec![{
        let mut p = persona("p1", None, "prompt");
        p.model = None;
        p.provider = None;
        p
    }];

    let global_a = GlobalAgentConfig {
        model: Some("model-a".to_string()),
        ..Default::default()
    };
    let global_b = GlobalAgentConfig {
        model: Some("model-b".to_string()),
        ..Default::default()
    };

    let hash_a = spawn_config_hash(&rec, &personas, &[], "wss://ws.example", &global_a);
    let hash_b = spawn_config_hash(&rec, &personas, &[], "wss://ws.example", &global_b);

    assert_ne!(
        hash_a, hash_b,
        "global model change must trip hash even without a model_env_var runtime"
    );
}

#[test]
fn linked_instance_selected_prompt_bytes_are_authoritative_at_hash_time() {
    let mut rec = record();
    rec.persona_id = Some("p1".into());
    rec.system_prompt = Some("stale prompt on record".into());

    let mut matching_bytes = rec.clone();
    matching_bytes.system_prompt = Some("live prompt".into());

    let personas = [persona("p1", Some("goose"), "live prompt")];

    assert_ne!(
        spawn_config_hash(
            &rec,
            &personas,
            &[],
            "wss://ws.example",
            &Default::default()
        ),
        spawn_config_hash(
            &matching_bytes,
            &personas,
            &[],
            "wss://ws.example",
            &Default::default()
        ),
        "selected prompt bytes must affect the hash of a linked instance"
    );
}

#[test]
fn display_name_edit_changes_hash() {
    // The spawn writes BUZZ_ACP_SESSION_TITLE from display_name-or-name, so a
    // rename must trip the badge: the running process keeps the old title
    // until it restarts, and the operator has to be told that.
    let rec = record();
    let mut renamed = record();
    renamed.display_name = Some("Fizz".into());
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&renamed, &[], &[], "wss://ws.example", &Default::default()),
        "a display-name rename changes the spawned session title and must badge"
    );
}

#[test]
fn name_edit_changes_hash_when_display_name_is_absent() {
    // With no display_name the title falls back to the unique handle, so the
    // handle is what the env write carries and what must be hashed.
    let rec = record();
    let mut renamed = record();
    renamed.name = "agent-2".into();
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&renamed, &[], &[], "wss://ws.example", &Default::default()),
        "the fallback title source must reach the hash too"
    );
}

#[test]
fn display_name_edit_does_not_change_hash_under_an_explicit_title_override() {
    // User env is written AFTER the Buzz-set title (last-wins), so an explicit
    // BUZZ_ACP_SESSION_TITLE is what the child actually runs with. Renaming the
    // record changes nothing about the spawned process, so badging it would be
    // a false restart prompt. The override itself still reaches the hash
    // through the effective env.
    let mut rec = record();
    rec.env_vars
        .insert("BUZZ_ACP_SESSION_TITLE".into(), "Pinned Title".into());
    let mut renamed = rec.clone();
    renamed.display_name = Some("Fizz".into());
    assert_eq!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&renamed, &[], &[], "wss://ws.example", &Default::default()),
        "a rename shadowed by an explicit title override must not badge"
    );
}

#[test]
fn title_override_edit_changes_hash() {
    // Counterpart to the test above: the override is not inert — editing it
    // changes what the child runs with and must badge.
    let mut rec = record();
    rec.env_vars
        .insert("BUZZ_ACP_SESSION_TITLE".into(), "Pinned Title".into());
    let mut edited = record();
    edited
        .env_vars
        .insert("BUZZ_ACP_SESSION_TITLE".into(), "Other Title".into());
    assert_ne!(
        spawn_config_hash(&rec, &[], &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&edited, &[], &[], "wss://ws.example", &Default::default()),
        "editing an explicit title override must badge"
    );
}

#[test]
fn linked_instance_prompt_model_provider_share_one_pinned_resolution() {
    let mut rec = record();
    rec.persona_id = Some("p1".into());
    rec.system_prompt = Some("stale".into());

    let before = [persona("p1", Some("goose"), "old definition prompt")];
    let after = [persona("p1", Some("goose"), "new definition prompt")];

    assert_eq!(
        spawn_config_hash(&rec, &before, &[], "wss://ws.example", &Default::default()),
        spawn_config_hash(&rec, &after, &[], "wss://ws.example", &Default::default()),
        "mutable definition prompt edits must not alter the selected resolution"
    );
}

// ── I2: definition args and env reach spawn_config_hash ──────────────────────
//
// These tests prove that editing a custom harness definition's args or env
// changes spawn_config_hash, which trips the "restart required" badge.
// They would fail if spawn_config_hash used only record.agent_args without
// falling back to definition args, or if resolve_effective_agent_env did not
// include definition env.

/// When a record has no instance args but the definition has default args,
/// changing the definition args changes the spawn hash. This would fail if
/// spawn_config_hash used only record.agent_args.
#[test]
fn spawn_hash_changes_when_definition_default_args_change() {
    use crate::managed_agents::custom_harnesses::{
        registry_test_lock, warm_harness_registry_from_dir,
    };
    use std::fs;
    use tempfile::tempdir;

    // The loaded-harness registry is process-global: a parallel test re-warming
    // it between the two hash computations makes both resolve to no-definition
    // and h1 == h2 (observed on Windows CI).
    let _lock = registry_test_lock();
    let dir = tempdir().unwrap();

    // Write v1 definition (args: ["--mode", "v1"]).
    fs::write(
        dir.path().join("my-def.json"),
        r#"{"id":"my-def","label":"My Def","command":"my-def-bin","args":["--mode","v1"]}"#,
    )
    .unwrap();
    warm_harness_registry_from_dir(Some(dir.path()));

    let mut r = record();
    r.runtime = Some("my-def".into());
    r.agent_args = vec![]; // no instance args → definition args are used

    let h1 = spawn_config_hash(&r, &[], &[], "ws://relay", &Default::default());

    // Update to v2 args and re-warm (simulating save + transactional refresh).
    fs::write(
        dir.path().join("my-def.json"),
        r#"{"id":"my-def","label":"My Def","command":"my-def-bin","args":["--mode","v2"]}"#,
    )
    .unwrap();
    warm_harness_registry_from_dir(Some(dir.path()));

    let h2 = spawn_config_hash(&r, &[], &[], "ws://relay", &Default::default());

    assert_ne!(
        h1, h2,
        "changing definition default args must change the spawn hash"
    );
}

/// When a definition has env vars, adding them changes the spawn hash. This
/// proves resolve_effective_agent_env includes definition env in the layering.
#[test]
fn spawn_hash_changes_when_definition_env_changes() {
    use crate::managed_agents::custom_harnesses::{
        registry_test_lock, warm_harness_registry_from_dir,
    };
    use std::fs;
    use tempfile::tempdir;

    // Serialize against parallel registry re-warms (see the args test above).
    let _lock = registry_test_lock();
    let dir = tempdir().unwrap();

    // Write definition without env.
    fs::write(
        dir.path().join("env-def.json"),
        r#"{"id":"env-def","label":"Env Def","command":"env-def-bin"}"#,
    )
    .unwrap();
    warm_harness_registry_from_dir(Some(dir.path()));

    let mut r = record();
    r.runtime = Some("env-def".into());

    let h1 = spawn_config_hash(&r, &[], &[], "ws://relay", &Default::default());

    // Update to include env and re-warm.
    fs::write(
        dir.path().join("env-def.json"),
        r#"{"id":"env-def","label":"Env Def","command":"env-def-bin","env":{"MY_FLAG":"1"}}"#,
    )
    .unwrap();
    warm_harness_registry_from_dir(Some(dir.path()));

    let h2 = spawn_config_hash(&r, &[], &[], "ws://relay", &Default::default());

    assert_ne!(h1, h2, "adding definition env must change the spawn hash");
}

/// Instance-level args win over definition default args (non-empty instance
/// args must NOT be overridden by the definition). The hash must match a record
/// that has the same effective args from either source.
#[test]
fn spawn_hash_instance_args_win_over_definition_args() {
    use crate::managed_agents::custom_harnesses::{
        registry_test_lock, warm_harness_registry_from_dir,
    };
    use std::fs;
    use tempfile::tempdir;

    // Serialize against parallel registry re-warms (see the args test above).
    let _lock = registry_test_lock();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("arg-def.json"),
        r#"{"id":"arg-def","label":"Arg Def","command":"arg-def-bin","args":["--def-arg"]}"#,
    )
    .unwrap();
    warm_harness_registry_from_dir(Some(dir.path()));

    let mut r_instance = record();
    r_instance.runtime = Some("arg-def".into());
    r_instance.agent_args = vec!["--instance-arg".to_string()];

    let mut r_no_instance = record();
    r_no_instance.runtime = Some("arg-def".into());
    r_no_instance.agent_args = vec![];

    let h_instance = spawn_config_hash(&r_instance, &[], &[], "ws://relay", &Default::default());
    let h_no_instance =
        spawn_config_hash(&r_no_instance, &[], &[], "ws://relay", &Default::default());

    assert_ne!(
        h_instance, h_no_instance,
        "instance args and definition args must produce different hashes"
    );
}
