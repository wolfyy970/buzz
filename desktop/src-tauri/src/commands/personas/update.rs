//! The persona edit command surface: `update_persona` (best-effort enqueue)
//! and the `update_persona_with` seam that `update_persona_and_publish` reuses
//! to await relay acceptance for the same save.

use tauri::AppHandle;

use crate::{
    app_state::AppState,
    managed_agents::{
        apply_persona_behavior, load_personas, save_personas, try_regenerate_nest, AgentDefinition,
        UpdatePersonaRequest,
    },
    util::now_iso,
};

use super::{pending, retain_persona_pending, trim_optional, trim_required};

/// Return value of the `update_persona` command. Uses flatten so all
/// `AgentDefinition` fields appear at the top level of the JSON response —
/// backward-compatible with callers that already destructure a raw persona object.
#[derive(Debug, serde::Serialize)]
pub struct UpdatePersonaResult {
    #[serde(flatten)]
    persona: AgentDefinition,
}

#[tauri::command]
pub async fn update_persona(
    input: UpdatePersonaRequest,
    app: AppHandle,
) -> Result<UpdatePersonaResult, String> {
    let (persona, ()) = update_persona_with(input, app, |app, state, persona| {
        retain_persona_pending(app, state, persona);
        Ok(())
    })
    .await?;
    Ok(UpdatePersonaResult { persona })
}

/// Save an edited persona and hand the saved record to `retain` while the store
/// lock is still held. Existing instances retain their selected identity and
/// configuration until an explicit template-version update targets them.
///
/// `retain` is the only difference between the two update commands:
/// [`update_persona`] enqueues best-effort, while
/// [`sharing::update_persona_and_publish`] prepares a strict publication and
/// returns the event so the caller can await relay acceptance.
pub(super) async fn update_persona_with<R: Send + 'static>(
    input: UpdatePersonaRequest,
    app: AppHandle,
    retain: impl FnOnce(&AppHandle, &AppState, &AgentDefinition) -> Result<R, String> + Send + 'static,
) -> Result<(AgentDefinition, R), String> {
    use tauri::Manager;

    let (result, retained) = tokio::task::spawn_blocking({
        let app = app.clone();
        move || -> Result<(AgentDefinition, R), String> {
            let state = app.state::<AppState>();
            let display_name = trim_required(&input.display_name, "Display name")?;
            let system_prompt = input.system_prompt.clone();
            let avatar_url = trim_optional(input.avatar_url);
            let runtime = trim_optional(input.runtime);
            let model = trim_optional(input.model);
            let provider = trim_optional(input.provider);

            let _store_guard = state
                .managed_agents_store_lock
                .lock()
                .map_err(|error| error.to_string())?;
            let mut personas = load_personas(&app)?;
            pending::project_active_persona_sharing(&app, &state, &mut personas);
            let persona = personas
                .iter_mut()
                .find(|record| record.id == input.id)
                .ok_or_else(|| format!("agent {} not found", input.id))?;

            persona.display_name = display_name;
            persona.avatar_url = avatar_url;
            persona.system_prompt = system_prompt;
            persona.runtime = runtime;
            persona.model = model;
            persona.provider = provider;
            persona.name_pool = input
                .name_pool
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if let Some(env_vars) = input.env_vars {
                crate::managed_agents::validate_user_env_keys(&env_vars)?;
                persona.env_vars = env_vars;
            }
            if let Some(tool_requirements) = input.tool_requirements {
                crate::managed_agents::project_connections::validate_tool_requirements(
                    &tool_requirements,
                )?;
                persona.tool_requirements = tool_requirements;
            }
            if let Some(skills) = input.skills {
                crate::managed_agents::validate_agent_skills(&skills)?;
                persona.skills = skills;
            }
            apply_persona_behavior(persona, input.behavior)?;
            persona.updated_at = now_iso();

            let result = persona.clone();
            save_personas(&app, &personas)?;

            let retained = retain(&app, &state, &result)?;
            try_regenerate_nest(&app);
            Ok((result, retained))
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))??;

    let mut public_result = result;
    public_result.published_version_env_vars = None;
    Ok((public_result, retained))
}
