//! Publish and load immutable agent template versions from Buzz-hosted Git.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use nostr::Tag;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tauri::{AppHandle, Manager};

use crate::{
    app_state::AppState,
    events,
    managed_agents::{
        load_personas, save_personas, validate_agent_definition_text, validate_agent_skills,
        AgentDefinition, AgentSkill, AgentTemplateVersionRef, AgentToolRequirement,
    },
};

use super::{
    project_git::first_output_line,
    project_git_exec::{build_git_auth_config, run_git, GitAuthConfig},
    retain_persona_pending,
};

pub(crate) const AGENT_TEMPLATE_REPO_ID: &str = "buzz-agent-templates";
pub(crate) const AGENT_TEMPLATE_REPO_PURPOSE: &str = "agent-templates";
pub(crate) const AGENT_TEMPLATE_CHANNEL_PURPOSE: &str = "Buzz agent template version storage";
const AGENT_TEMPLATE_CHANNEL_NAME: &str = "buzz-agent-template-versions";
const AGENT_TEMPLATE_CHANNEL_DESCRIPTION: &str =
    "Private storage used by Buzz for agent template versions.";
const AGENT_TEMPLATE_VERSION_SCHEMA: u32 = 1;
const MAX_AGENT_TEMPLATE_ARTIFACT_BYTES: usize = 256 * 1024;
const AGENT_TEMPLATE_CHANNEL_NAMESPACE: uuid::Uuid =
    uuid::uuid!("b76a37f9-999c-5d08-bfba-0723f71b32bb");

/// Secret-free immutable contents stored in Buzz-hosted Git.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentTemplateArtifactV1 {
    schema_version: u32,
    template_id: String,
    display_name: String,
    avatar_url: Option<String>,
    system_prompt: String,
    runtime: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    name_pool: Vec<String>,
    respond_to: Option<String>,
    respond_to_allowlist: Vec<String>,
    parallelism: Option<u32>,
    tool_requirements: Vec<AgentToolRequirement>,
    skills: Vec<AgentSkill>,
}

impl AgentTemplateArtifactV1 {
    fn from_definition(definition: &AgentDefinition) -> Result<Self, String> {
        validate_agent_definition_text(&definition.display_name, &definition.system_prompt)?;
        validate_agent_skills(&definition.skills)?;
        crate::managed_agents::project_connections::validate_tool_requirements(
            &definition.tool_requirements,
        )?;
        let mut tool_requirements = definition.tool_requirements.clone();
        tool_requirements.sort_by(|left, right| left.id.cmp(&right.id));
        let mut skills = definition.skills.clone();
        skills.sort_by(|left, right| left.name.cmp(&right.name));
        for skill in &mut skills {
            skill
                .files
                .sort_by(|left, right| left.path.cmp(&right.path));
        }
        let mut respond_to_allowlist = definition.respond_to_allowlist.clone();
        respond_to_allowlist.sort();
        respond_to_allowlist.dedup();
        Ok(Self {
            schema_version: AGENT_TEMPLATE_VERSION_SCHEMA,
            template_id: definition.id.clone(),
            display_name: definition.display_name.clone(),
            avatar_url: definition.avatar_url.clone(),
            system_prompt: definition.system_prompt.clone(),
            runtime: definition.runtime.clone(),
            model: definition.model.clone(),
            provider: definition.provider.clone(),
            name_pool: definition.name_pool.clone(),
            respond_to: definition.respond_to.clone(),
            respond_to_allowlist,
            parallelism: definition.parallelism,
            tool_requirements,
            skills,
        })
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("Could not serialize the template version: {error}"))?;
        if bytes.len() > MAX_AGENT_TEMPLATE_ARTIFACT_BYTES {
            return Err(format!(
                "The template version exceeds {MAX_AGENT_TEMPLATE_ARTIFACT_BYTES} bytes."
            ));
        }
        Ok(bytes)
    }

    fn digest(&self) -> Result<String, String> {
        Ok(hex::encode(Sha256::digest(self.canonical_bytes()?)))
    }

    fn into_definition(
        self,
        local_head: &AgentDefinition,
        version: &AgentTemplateVersionRef,
    ) -> AgentDefinition {
        AgentDefinition {
            id: self.template_id,
            display_name: self.display_name,
            avatar_url: self.avatar_url,
            system_prompt: self.system_prompt,
            runtime: self.runtime,
            model: self.model,
            provider: self.provider,
            name_pool: self.name_pool,
            is_builtin: local_head.is_builtin,
            is_active: local_head.is_active,
            shared: local_head.shared,
            source_team: local_head.source_team.clone(),
            source_team_persona_slug: local_head.source_team_persona_slug.clone(),
            catalog_source: local_head.catalog_source.clone(),
            env_vars: local_head
                .published_version_env_vars
                .clone()
                .unwrap_or_default(),
            tool_requirements: self.tool_requirements,
            skills: self.skills,
            published_version: Some(version.clone()),
            published_version_env_vars: local_head.published_version_env_vars.clone(),
            respond_to: self.respond_to,
            respond_to_allowlist: self.respond_to_allowlist,
            parallelism: self.parallelism,
            created_at: local_head.created_at.clone(),
            updated_at: local_head.updated_at.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishAgentTemplateVersionRequest {
    pub persona_id: String,
    pub expected_updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishAgentTemplateVersionResponse {
    pub persona_id: String,
    pub persona_name: String,
    pub version: AgentTemplateVersionRef,
}

/// Deterministic private storage channel for one owner on one relay.
pub(crate) fn agent_template_storage_channel_uuid(
    relay_scope: &str,
    owner_pubkey: &str,
) -> uuid::Uuid {
    let name = format!(
        "agent-template-storage:v1:{}:{}",
        relay_scope.trim(),
        owner_pubkey.to_ascii_lowercase()
    );
    uuid::Uuid::new_v5(&AGENT_TEMPLATE_CHANNEL_NAMESPACE, name.as_bytes())
}

pub(crate) fn is_internal_agent_template_channel(
    channel: &crate::models::ChannelInfo,
    relay_scope: &str,
    owner_pubkey: &str,
) -> bool {
    channel.id == agent_template_storage_channel_uuid(relay_scope, owner_pubkey).to_string()
        && channel.name == AGENT_TEMPLATE_CHANNEL_NAME
        && channel.visibility == "private"
        && channel.channel_type == "stream"
        && channel.purpose.as_deref() == Some(AGENT_TEMPLATE_CHANNEL_PURPOSE)
}

fn validate_agent_template_storage_boundary(
    channel: &crate::models::ChannelInfo,
    member_pubkeys: &[String],
    owner_pubkey: &str,
) -> Result<(), String> {
    let owner = owner_pubkey.to_ascii_lowercase();
    let mut members = member_pubkeys
        .iter()
        .map(|member| member.to_ascii_lowercase())
        .collect::<Vec<_>>();
    members.sort();
    members.dedup();
    if channel.name != AGENT_TEMPLATE_CHANNEL_NAME
        || channel.visibility != "private"
        || channel.channel_type != "stream"
        || members != [owner]
    {
        return Err(
            "Template version storage is not private to this Buzz identity. Publishing was stopped."
                .to_string(),
        );
    }
    Ok(())
}

async fn verify_existing_agent_template_storage(
    state: &AppState,
    channel_id: &str,
    owner_pubkey: &str,
) -> Result<(), String> {
    let metadata_filter = serde_json::json!({
        "kinds": [39000],
        "#d": [channel_id],
        "limit": 1,
    });
    let members_filter = serde_json::json!({
        "kinds": [39002],
        "#d": [channel_id],
        "limit": 1,
    });
    let metadata_filters = [metadata_filter];
    let members_filters = [members_filter];
    let (metadata_events, membership_events) = tokio::try_join!(
        crate::relay::query_relay(state, &metadata_filters),
        crate::relay::query_relay(state, &members_filters),
    )
    .map_err(|error| format!("Could not verify template version storage: {error}"))?;
    let metadata_event = metadata_events
        .iter()
        .max_by_key(|event| event.created_at)
        .ok_or_else(|| "Could not verify template version storage metadata.".to_string())?;
    let membership_event = membership_events
        .iter()
        .max_by_key(|event| event.created_at)
        .ok_or_else(|| "Could not verify template version storage members.".to_string())?;
    let channel = crate::nostr_convert::channel_info_from_event(metadata_event, None, Some(true))
        .map_err(|error| format!("Could not verify template version storage: {error}"))?;
    let members = crate::nostr_convert::channel_members_from_event(membership_event)
        .map_err(|error| format!("Could not verify template version storage: {error}"))?
        .members
        .into_iter()
        .map(|member| member.pubkey)
        .collect::<Vec<_>>();
    validate_agent_template_storage_boundary(&channel, &members, owner_pubkey)
}

fn tag(values: &[&str]) -> Result<Tag, String> {
    Tag::parse(values.iter().copied()).map_err(|error| format!("invalid repository tag: {error}"))
}

async fn ensure_agent_template_git_storage(
    state: &AppState,
    keys: &nostr::Keys,
) -> Result<(String, String), String> {
    let relay_scope = crate::relay::relay_api_base_url_with_override(state);
    let owner = keys.public_key().to_hex().to_ascii_lowercase();
    let channel_id = agent_template_storage_channel_uuid(&relay_scope, &owner);
    let create = events::build_create_channel(
        channel_id,
        AGENT_TEMPLATE_CHANNEL_NAME,
        "private",
        "stream",
        Some(AGENT_TEMPLATE_CHANNEL_DESCRIPTION),
        None,
    )?;
    let created_fresh = match crate::relay::submit_event_with_keys(create, state, keys, None).await
    {
        Ok(_) => {
            state.mark_pending_owned_channel(&owner, &channel_id.to_string());
            true
        }
        Err(error)
            if error.contains("relay rejected event:")
                && error.contains("duplicate: channel already exists") =>
        {
            false
        }
        Err(error) => {
            return Err(format!(
                "Could not prepare template version storage: {error}"
            ))
        }
    };
    if !created_fresh {
        verify_existing_agent_template_storage(state, &channel_id.to_string(), &owner).await?;
    }
    // A newly accepted create is atomically private and owner-only. Its
    // metadata and membership projections may not be queryable immediately,
    // so the read-back boundary check applies on every later publish.
    crate::relay::submit_event_with_keys(
        events::build_set_purpose(channel_id, AGENT_TEMPLATE_CHANNEL_PURPOSE)?,
        state,
        keys,
        None,
    )
    .await
    .map_err(|error| format!("Could not prepare template version storage: {error}"))?;

    let tags = vec![
        tag(&["name", "Buzz agent template versions"])?,
        tag(&[
            "description",
            "Immutable agent template versions managed by Buzz.",
        ])?,
        tag(&["buzz-purpose", AGENT_TEMPLATE_REPO_PURPOSE])?,
        tag(&["buzz-channel", &channel_id.to_string()])?,
    ];
    let announcement =
        buzz_sdk_pkg::build_repo_announcement_with_tags(AGENT_TEMPLATE_REPO_ID, "", tags)
            .map_err(|error| format!("Could not build template repository metadata: {error}"))?;
    crate::relay::submit_event_with_keys(announcement, state, keys, None)
        .await
        .map_err(|error| format!("Could not prepare template version storage: {error}"))?;

    let repo_address = format!("30617:{owner}:{AGENT_TEMPLATE_REPO_ID}");
    let clone_url = format!(
        "{}/git/{owner}/{AGENT_TEMPLATE_REPO_ID}",
        relay_scope.trim_end_matches('/')
    );
    Ok((repo_address, clone_url))
}

fn safe_artifact_path(template_id: &str, digest: &str) -> Result<String, String> {
    if template_id.is_empty()
        || template_id.len() > 64
        || !template_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err("Template id cannot be used as a version path.".to_string());
    }
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Template version digest is invalid.".to_string());
    }
    Ok(format!(
        "templates/{template_id}/versions/{digest}/template.json"
    ))
}

fn ensure_safe_parent(repo: &Path, artifact_path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(artifact_path);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Template version path is unsafe.".to_string());
    }
    let parent = relative
        .parent()
        .ok_or_else(|| "Template version path has no parent.".to_string())?;
    let mut cursor = repo.to_path_buf();
    for component in parent.components() {
        let Component::Normal(component) = component else {
            return Err("Template version path is unsafe.".to_string());
        };
        cursor.push(component);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Template repository contains an unsafe link.".to_string())
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("Template repository path is not a directory.".to_string())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&cursor).map_err(|error| {
                    format!("Could not create the template version directory: {error}")
                })?;
            }
            Err(error) => {
                return Err(format!(
                    "Could not inspect the template version directory: {error}"
                ))
            }
        }
    }
    let file = repo.join(relative);
    if fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Template repository contains an unsafe link.".to_string());
    }
    Ok(file)
}

fn publish_artifact_to_repo(
    clone_url: &str,
    repo_address: &str,
    artifact: &AgentTemplateArtifactV1,
    auth: &GitAuthConfig,
) -> Result<AgentTemplateVersionRef, String> {
    let bytes = artifact.canonical_bytes()?;
    let digest = hex::encode(Sha256::digest(&bytes));
    let artifact_path = safe_artifact_path(&artifact.template_id, &digest)?;
    let temp = tempfile::tempdir()
        .map_err(|error| format!("Could not create a template version checkout: {error}"))?;
    let checkout = temp.path().join("repository");
    let checkout_text = checkout
        .to_str()
        .ok_or_else(|| "Template version checkout path is not UTF-8.".to_string())?;
    run_git(
        &["clone", "--no-tags", "--", clone_url, checkout_text],
        None,
        auth,
    )
    .map_err(|error| format!("Could not open template version storage: {error}"))?;
    let remote_main_exists = run_git(
        &["rev-parse", "--verify", "refs/remotes/origin/main"],
        Some(&checkout),
        auth,
    )
    .is_ok();
    let checkout_args = if remote_main_exists {
        vec!["checkout", "-B", "main", "refs/remotes/origin/main"]
    } else {
        vec!["checkout", "--orphan", "main"]
    };
    run_git(&checkout_args, Some(&checkout), auth)
        .map_err(|error| format!("Could not prepare the template version branch: {error}"))?;
    let file = ensure_safe_parent(&checkout, &artifact_path)?;
    fs::write(&file, bytes)
        .map_err(|error| format!("Could not write the template version: {error}"))?;
    run_git(&["add", "--", &artifact_path], Some(&checkout), auth)
        .map_err(|error| format!("Could not stage the template version: {error}"))?;
    run_git(
        &[
            "-c",
            "user.name=Buzz",
            "-c",
            "user.email=agent-templates@buzz.invalid",
            "commit",
            "--no-gpg-sign",
            "--allow-empty",
            "-m",
            "Publish agent template version",
        ],
        Some(&checkout),
        auth,
    )
    .map_err(|error| format!("Could not commit the template version: {error}"))?;
    let commit_oid = first_output_line(&run_git(&["rev-parse", "HEAD"], Some(&checkout), auth)?)
        .ok_or_else(|| "Git did not return the template version commit.".to_string())?;
    run_git(
        &["push", "--end-of-options", "origin", "HEAD:refs/heads/main"],
        Some(&checkout),
        auth,
    )
    .map_err(|error| format!("Could not publish the template version: {error}"))?;
    let version = AgentTemplateVersionRef {
        repo_address: repo_address.to_string(),
        commit_oid,
        artifact_path,
        artifact_sha256: digest,
    };
    version.validate()?;
    Ok(version)
}

fn clone_url_for_version(
    state: &AppState,
    version: &AgentTemplateVersionRef,
) -> Result<String, String> {
    version.validate()?;
    let owner = version
        .repo_address
        .split(':')
        .nth(1)
        .ok_or_else(|| "Template version repository owner is missing.".to_string())?;
    let active_owner = state.signing_keys()?.public_key().to_hex();
    if !owner.eq_ignore_ascii_case(&active_owner) {
        return Err("This template version belongs to another Buzz identity.".to_string());
    }
    Ok(format!(
        "{}/git/{owner}/{AGENT_TEMPLATE_REPO_ID}",
        crate::relay::relay_api_base_url_with_override(state).trim_end_matches('/')
    ))
}

fn load_artifact_from_repo(
    clone_url: &str,
    version: &AgentTemplateVersionRef,
    auth: &GitAuthConfig,
) -> Result<AgentTemplateArtifactV1, String> {
    version.validate()?;
    let temp = tempfile::tempdir()
        .map_err(|error| format!("Could not create a template version checkout: {error}"))?;
    let checkout = temp.path().join("repository");
    let checkout_text = checkout
        .to_str()
        .ok_or_else(|| "Template version checkout path is not UTF-8.".to_string())?;
    run_git(
        &[
            "clone",
            "--no-checkout",
            "--no-tags",
            "--",
            clone_url,
            checkout_text,
        ],
        None,
        auth,
    )
    .map_err(|error| format!("Could not load the template version: {error}"))?;
    let object = format!("{}:{}", version.commit_oid, version.artifact_path);
    let content = run_git(
        &["show", "--no-ext-diff", "--no-textconv", &object],
        Some(&checkout),
        auth,
    )
    .map_err(|error| format!("Could not read the template version: {error}"))?;
    if content.len() > MAX_AGENT_TEMPLATE_ARTIFACT_BYTES {
        return Err("The template version artifact is too large.".to_string());
    }
    let actual_digest = hex::encode(Sha256::digest(content.as_bytes()));
    if actual_digest != version.artifact_sha256 {
        return Err("The template version failed its integrity check.".to_string());
    }
    let artifact: AgentTemplateArtifactV1 = serde_json::from_str(&content)
        .map_err(|error| format!("The template version is not valid JSON: {error}"))?;
    if artifact.schema_version != AGENT_TEMPLATE_VERSION_SCHEMA {
        return Err(format!(
            "Template version schema {} is not supported.",
            artifact.schema_version
        ));
    }
    validate_agent_definition_text(&artifact.display_name, &artifact.system_prompt).map_err(
        |error| format!("The template version contains unsafe definition text: {error}"),
    )?;
    validate_agent_skills(&artifact.skills)?;
    crate::managed_agents::project_connections::validate_tool_requirements(
        &artifact.tool_requirements,
    )?;
    if artifact.digest()? != version.artifact_sha256 {
        return Err("The template version is not canonical.".to_string());
    }
    Ok(artifact)
}

pub(crate) fn load_agent_template_version(
    app: &AppHandle,
    persona: &AgentDefinition,
    version: &AgentTemplateVersionRef,
) -> Result<AgentDefinition, String> {
    let state = app.state::<AppState>();
    let clone_url = clone_url_for_version(&state, version)?;
    let auth = build_git_auth_config(&state)?;
    let artifact = load_artifact_from_repo(&clone_url, version, &auth)?;
    if artifact.template_id != persona.id {
        return Err("The template version belongs to a different agent template.".to_string());
    }
    Ok(artifact.into_definition(persona, version))
}

#[tauri::command]
pub async fn publish_agent_template_version(
    input: PublishAgentTemplateVersionRequest,
    app: AppHandle,
) -> Result<PublishAgentTemplateVersionResponse, String> {
    let state = app.state::<AppState>();
    let (persona, previous_version) = {
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let personas = load_personas(&app)?;
        let persona = personas
            .iter()
            .find(|persona| persona.id == input.persona_id)
            .cloned()
            .ok_or_else(|| format!("Agent template {} no longer exists.", input.persona_id))?;
        if persona.updated_at != input.expected_updated_at {
            return Err(
                "This agent template changed before the version was published. Review it and try again."
                    .to_string(),
            );
        }
        (persona.clone(), persona.published_version)
    };
    let artifact = AgentTemplateArtifactV1::from_definition(&persona)?;
    let keys = state.signing_keys()?;
    let (repo_address, clone_url) = ensure_agent_template_git_storage(&state, &keys).await?;
    let auth = build_git_auth_config(&state)?;
    let version = tokio::task::spawn_blocking(move || {
        publish_artifact_to_repo(&clone_url, &repo_address, &artifact, &auth)
    })
    .await
    .map_err(|error| format!("Template version task failed: {error}"))??;

    let persona_name = {
        let _store = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let mut personas = load_personas(&app)?;
        let current = personas
            .iter_mut()
            .find(|current| current.id == input.persona_id)
            .ok_or_else(|| format!("Agent template {} no longer exists.", input.persona_id))?;
        if current.updated_at != input.expected_updated_at {
            return Err(
                "This agent template changed before the version was published. Review it and try again."
                    .to_string(),
            );
        }
        if current.published_version != previous_version {
            return Err(
                "A newer template version was published first. Refresh and try again.".to_string(),
            );
        }
        current.published_version = Some(version.clone());
        current.published_version_env_vars = Some(persona.env_vars);
        let persona_name = current.display_name.clone();
        let retained = current.clone();
        save_personas(&app, &personas)?;
        retain_persona_pending(&app, &state, &retained);
        persona_name
    };

    Ok(PublishAgentTemplateVersionResponse {
        persona_id: input.persona_id,
        persona_name,
        version,
    })
}

#[cfg(test)]
mod tests;
