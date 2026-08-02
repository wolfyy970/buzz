use super::*;
use crate::commands::project_git_exec::{build_test_git_auth_config, run_git};
use std::collections::BTreeMap;

fn definition() -> AgentDefinition {
    AgentDefinition {
        id: "analytics".to_string(),
        display_name: "Analytics".to_string(),
        avatar_url: None,
        system_prompt: "Analyze weekly performance.".to_string(),
        runtime: Some("codex".to_string()),
        model: Some("gpt-5".to_string()),
        provider: Some("openai".to_string()),
        name_pool: vec!["Ada".to_string()],
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: BTreeMap::from([(
            "ANALYTICS_API_KEY".to_string(),
            "must-not-enter-git".to_string(),
        )]),
        tool_requirements: Vec::new(),
        skills: Vec::new(),
        published_version: None,
        published_version_env_vars: None,
        respond_to: None,
        respond_to_allowlist: Vec::new(),
        parallelism: None,
        created_at: "created".to_string(),
        updated_at: "updated".to_string(),
    }
}

fn local_repo_fixture() -> (tempfile::TempDir, std::path::PathBuf, String, GitAuthConfig) {
    let auth = build_test_git_auth_config().expect("test git auth");
    let root = tempfile::tempdir().expect("fixture root");
    let remote = root.path().join("remote.git");
    let remote_text = remote.to_str().expect("remote path").to_string();
    run_git(&["init", "--bare", "--", &remote_text], None, &auth).expect("bare repository");
    (root, remote, remote_text, auth)
}

fn repo_address() -> String {
    format!("30617:{}:{AGENT_TEMPLATE_REPO_ID}", "a".repeat(64))
}

#[test]
fn artifact_is_deterministic_and_excludes_environment_secrets() {
    let definition = definition();
    let first = AgentTemplateArtifactV1::from_definition(&definition).expect("artifact");
    let second = AgentTemplateArtifactV1::from_definition(&definition).expect("artifact");
    assert_eq!(
        first.canonical_bytes().unwrap(),
        second.canonical_bytes().unwrap()
    );
    assert_eq!(first.digest().unwrap(), second.digest().unwrap());
    let json = String::from_utf8(first.canonical_bytes().unwrap()).unwrap();
    assert!(!json.contains("ANALYTICS_API_KEY"));
    assert!(!json.contains("must-not-enter-git"));
    let event_json = serde_json::to_string(
        &crate::managed_agents::persona_events::persona_event_content(&definition),
    )
    .unwrap();
    assert!(!event_json.contains("ANALYTICS_API_KEY"));
    assert!(!event_json.contains("must-not-enter-git"));
}

#[test]
fn artifact_rejects_unreviewable_definition_text_before_publish() {
    let mut unsafe_definition = definition();
    unsafe_definition.system_prompt = "Analyze\u{200B} weekly performance.".to_string();

    let error = AgentTemplateArtifactV1::from_definition(&unsafe_definition)
        .expect_err("invisible instructions must not enter an immutable version");

    assert!(error.contains("U+200B"));
}

#[test]
fn loading_an_immutable_artifact_revalidates_definition_text() {
    let (_root, _remote, clone_url, auth) = local_repo_fixture();
    let mut artifact = AgentTemplateArtifactV1::from_definition(&definition()).unwrap();
    artifact.system_prompt = "Analyze\u{202E} weekly performance.".to_string();
    let version = publish_artifact_to_repo(&clone_url, &repo_address(), &artifact, &auth).unwrap();

    let error = load_artifact_from_repo(&clone_url, &version, &auth)
        .expect_err("loaded version text must be safe before activation");

    assert!(error.contains("unsafe definition text"));
    assert!(error.contains("U+202E"));
}

#[test]
fn exact_commit_and_digest_survive_a_newer_publish() {
    let (_root, _remote, clone_url, auth) = local_repo_fixture();
    let first_artifact = AgentTemplateArtifactV1::from_definition(&definition()).unwrap();
    let first =
        publish_artifact_to_repo(&clone_url, &repo_address(), &first_artifact, &auth).unwrap();

    let mut edited = definition();
    edited.system_prompt = "A newer version.".to_string();
    let second_artifact = AgentTemplateArtifactV1::from_definition(&edited).unwrap();
    let second =
        publish_artifact_to_repo(&clone_url, &repo_address(), &second_artifact, &auth).unwrap();
    assert_ne!(first.commit_oid, second.commit_oid);
    assert_eq!(
        load_artifact_from_repo(&clone_url, &first, &auth).unwrap(),
        first_artifact
    );
    assert_eq!(
        load_artifact_from_repo(&clone_url, &second, &auth).unwrap(),
        second_artifact
    );
}

#[test]
fn publishing_the_same_public_content_still_creates_a_new_version() {
    let (_root, _remote, clone_url, auth) = local_repo_fixture();
    let artifact = AgentTemplateArtifactV1::from_definition(&definition()).unwrap();
    let first = publish_artifact_to_repo(&clone_url, &repo_address(), &artifact, &auth).unwrap();
    let second = publish_artifact_to_repo(&clone_url, &repo_address(), &artifact, &auth).unwrap();

    assert_ne!(first.commit_oid, second.commit_oid);
    assert_eq!(first.artifact_sha256, second.artifact_sha256);
    assert_eq!(first.artifact_path, second.artifact_path);
}

#[test]
fn builtin_template_ids_publish_through_a_safe_hashed_directory() {
    let (_root, _remote, clone_url, auth) = local_repo_fixture();
    let mut builtin = definition();
    builtin.id = "builtin:fizz".to_string();
    let artifact = AgentTemplateArtifactV1::from_definition(&builtin).unwrap();

    let version = publish_artifact_to_repo(&clone_url, &repo_address(), &artifact, &auth).unwrap();

    let template_path_component = version.artifact_path.split('/').nth(1).unwrap();
    assert_eq!(template_path_component.len(), 64);
    assert!(template_path_component
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
    assert!(!version.artifact_path.contains(':'));
    assert_eq!(
        load_artifact_from_repo(&clone_url, &version, &auth).unwrap(),
        artifact
    );
}

#[test]
fn altered_digest_and_path_are_rejected() {
    let (_root, _remote, clone_url, auth) = local_repo_fixture();
    let artifact = AgentTemplateArtifactV1::from_definition(&definition()).unwrap();
    let version = publish_artifact_to_repo(&clone_url, &repo_address(), &artifact, &auth).unwrap();

    let mut altered = version.clone();
    altered.artifact_sha256 = "f".repeat(64);
    assert!(load_artifact_from_repo(&clone_url, &altered, &auth)
        .unwrap_err()
        .contains("integrity"));

    let mut altered = version;
    altered.artifact_path = "../secret".to_string();
    assert!(load_artifact_from_repo(&clone_url, &altered, &auth).is_err());
}

#[cfg(unix)]
#[test]
fn publish_refuses_a_symlinked_artifact_parent() {
    use std::os::unix::fs::symlink;

    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join("templates")).unwrap();
    symlink("/tmp", repo.path().join("templates/analytics")).unwrap();
    let path = format!(
        "templates/analytics/versions/{}/template.json",
        "a".repeat(64)
    );
    assert!(ensure_safe_parent(repo.path(), &path)
        .unwrap_err()
        .contains("unsafe link"));
}

#[test]
fn storage_channel_identity_is_owner_relay_and_purpose_scoped() {
    let owner = "a".repeat(64);
    let channel = crate::models::ChannelInfo {
        id: agent_template_storage_channel_uuid("https://relay.example", &owner).to_string(),
        name: AGENT_TEMPLATE_CHANNEL_NAME.to_string(),
        channel_type: "stream".to_string(),
        visibility: "private".to_string(),
        description: AGENT_TEMPLATE_CHANNEL_DESCRIPTION.to_string(),
        topic: None,
        purpose: Some(AGENT_TEMPLATE_CHANNEL_PURPOSE.to_string()),
        member_count: 1,
        member_pubkeys: vec![owner.clone()],
        last_message_at: None,
        archived_at: None,
        participants: Vec::new(),
        participant_pubkeys: Vec::new(),
        is_member: true,
        ttl_seconds: None,
        ttl_deadline: None,
    };
    assert!(is_internal_agent_template_channel(
        &channel,
        "https://relay.example",
        &owner
    ));
    assert!(!is_internal_agent_template_channel(
        &channel,
        "https://other.example",
        &owner
    ));
    let mut wrong_purpose = channel;
    wrong_purpose.purpose = Some("user project".to_string());
    assert!(!is_internal_agent_template_channel(
        &wrong_purpose,
        "https://relay.example",
        &owner
    ));
    let mut wrong_visibility = wrong_purpose;
    wrong_visibility.purpose = Some(AGENT_TEMPLATE_CHANNEL_PURPOSE.to_string());
    wrong_visibility.visibility = "open".to_string();
    assert!(!is_internal_agent_template_channel(
        &wrong_visibility,
        "https://relay.example",
        &owner
    ));
}

#[test]
fn existing_storage_must_remain_private_and_owner_only() {
    let owner = "a".repeat(64);
    let channel = crate::models::ChannelInfo {
        id: agent_template_storage_channel_uuid("https://relay.example", &owner).to_string(),
        name: AGENT_TEMPLATE_CHANNEL_NAME.to_string(),
        channel_type: "stream".to_string(),
        visibility: "private".to_string(),
        description: AGENT_TEMPLATE_CHANNEL_DESCRIPTION.to_string(),
        topic: None,
        purpose: Some(AGENT_TEMPLATE_CHANNEL_PURPOSE.to_string()),
        member_count: 1,
        member_pubkeys: vec![owner.clone()],
        last_message_at: None,
        archived_at: None,
        participants: Vec::new(),
        participant_pubkeys: Vec::new(),
        is_member: true,
        ttl_seconds: None,
        ttl_deadline: None,
    };

    assert!(validate_agent_template_storage_boundary(
        &channel,
        std::slice::from_ref(&owner),
        &owner
    )
    .is_ok());

    let mut foreign_member = vec![owner.clone(), "b".repeat(64)];
    assert!(validate_agent_template_storage_boundary(&channel, &foreign_member, &owner).is_err());
    foreign_member.remove(0);
    assert!(validate_agent_template_storage_boundary(&channel, &foreign_member, &owner).is_err());

    let mut open_channel = channel;
    open_channel.visibility = "open".to_string();
    assert!(validate_agent_template_storage_boundary(
        &open_channel,
        std::slice::from_ref(&owner),
        &owner
    )
    .is_err());
}
