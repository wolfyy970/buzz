//! Explicit separation between immutable template pins and agent-only edits.

use crate::managed_agents::{validate_agent_skills, AgentSkill, ManagedAgentRecord};

pub(crate) struct AgentConfigurationUpdate {
    pub(crate) model: Option<Option<String>>,
    pub(crate) reset_model_to_template: bool,
    pub(crate) provider: Option<Option<String>>,
    pub(crate) reset_provider_to_template: bool,
    pub(crate) system_prompt: Option<Option<String>>,
    pub(crate) reset_system_prompt_to_template: bool,
    pub(crate) skills: Option<Vec<AgentSkill>>,
    pub(crate) reset_skills_to_template: bool,
}

pub(crate) fn apply_agent_configuration_update(
    record: &mut ManagedAgentRecord,
    update: AgentConfigurationUpdate,
) -> Result<(), String> {
    let AgentConfigurationUpdate {
        model,
        reset_model_to_template,
        provider,
        reset_provider_to_template,
        system_prompt,
        reset_system_prompt_to_template,
        skills,
        reset_skills_to_template,
    } = update;
    if reset_model_to_template && model.is_some() {
        return Err(
            "Choose either an agent-only model or Reset to template, not both.".to_string(),
        );
    }
    if reset_provider_to_template && provider.is_some() {
        return Err(
            "Choose either an agent-only provider or Reset to template, not both.".to_string(),
        );
    }
    if reset_system_prompt_to_template && system_prompt.is_some() {
        return Err(
            "Choose either agent-only instructions or Reset to template, not both.".to_string(),
        );
    }
    if reset_skills_to_template && skills.is_some() {
        return Err("Choose either agent-only Skills or Reset to template, not both.".to_string());
    }
    if let Some(skills) = skills.as_ref() {
        // Validate before mutating any field so a rejected bundle cannot leave
        // a partially applied instructions update behind.
        validate_agent_skills(skills)?;
    }

    if record.persona_id.is_some() {
        if reset_model_to_template {
            record.model_override = None;
        } else if let Some(model) = model {
            record.model_override = Some(model.unwrap_or_default());
        }
        if reset_provider_to_template {
            record.provider_override = None;
        } else if let Some(provider) = provider {
            record.provider_override = Some(provider.unwrap_or_default());
        }
        if reset_system_prompt_to_template {
            record.system_prompt_override = None;
        } else if let Some(prompt) = system_prompt {
            record.system_prompt_override = Some(prompt.unwrap_or_default());
        }
    } else {
        if let Some(model) = model {
            record.model = model;
        }
        if let Some(provider) = provider {
            record.provider = provider;
        }
        if reset_system_prompt_to_template {
            record.system_prompt_override = None;
        } else if let Some(prompt) = system_prompt {
            record.system_prompt = prompt;
        }
    }

    if reset_skills_to_template {
        record.skill_overrides = None;
    } else if let Some(skills) = skills {
        record.skill_overrides = Some(skills);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_agents::AgentSkillFile;

    fn valid_skill(name: &str) -> AgentSkill {
        let description = "Analyze a bounded dataset.";
        AgentSkill {
            name: name.to_string(),
            description: description.to_string(),
            files: vec![AgentSkillFile {
                path: "SKILL.md".to_string(),
                content: format!(
                    "---\nname: {name}\ndescription: {description}\n---\n\nUse the provided data.\n"
                ),
            }],
        }
    }

    fn linked_record() -> ManagedAgentRecord {
        let mut record: ManagedAgentRecord = serde_json::from_str(
            r#"{
                "pubkey":"agent","name":"Agent","persona_id":"template",
                "relay_url":"","acp_command":"","agent_command":"","agent_args":[],
                "mcp_command":"","turn_timeout_seconds":0,"parallelism":1,
                "system_prompt":"Template instructions","model":"template-model",
                "provider":"template-provider","env_vars":{},"created_at":"","updated_at":""
            }"#,
        )
        .expect("linked record");
        record.pinned_skills = vec![valid_skill("template-skill")];
        record
    }

    #[test]
    fn linked_private_edits_preserve_template_pins_and_can_reset() {
        let mut record = linked_record();
        let original_prompt = record.system_prompt.clone();
        let original_skills = record.pinned_skills.clone();

        apply_agent_configuration_update(
            &mut record,
            AgentConfigurationUpdate {
                model: Some(Some("ignored-model".into())),
                reset_model_to_template: false,
                provider: Some(Some("ignored-provider".into())),
                reset_provider_to_template: false,
                system_prompt: Some(Some("Only this agent".into())),
                reset_system_prompt_to_template: false,
                skills: Some(vec![valid_skill("agent-skill")]),
                reset_skills_to_template: false,
            },
        )
        .expect("private edit");

        assert_eq!(record.system_prompt, original_prompt);
        assert_eq!(record.pinned_skills, original_skills);
        assert_eq!(
            record.system_prompt_override.as_deref(),
            Some("Only this agent")
        );
        assert_eq!(
            record.skill_overrides.as_ref().unwrap()[0].name,
            "agent-skill"
        );
        assert_eq!(record.model.as_deref(), Some("template-model"));
        assert_eq!(record.provider.as_deref(), Some("template-provider"));
        assert_eq!(record.model_override.as_deref(), Some("ignored-model"));
        assert_eq!(
            record.provider_override.as_deref(),
            Some("ignored-provider")
        );

        apply_agent_configuration_update(
            &mut record,
            AgentConfigurationUpdate {
                model: None,
                reset_model_to_template: true,
                provider: None,
                reset_provider_to_template: true,
                system_prompt: None,
                reset_system_prompt_to_template: true,
                skills: None,
                reset_skills_to_template: true,
            },
        )
        .expect("reset");
        assert!(record.model_override.is_none());
        assert!(record.provider_override.is_none());
        assert!(record.system_prompt_override.is_none());
        assert!(record.skill_overrides.is_none());
    }

    #[test]
    fn invalid_skill_rejects_the_entire_private_edit() {
        let mut record = linked_record();
        let mut invalid = valid_skill("agent-skill");
        invalid.files[0].path = "../SKILL.md".to_string();

        let error = apply_agent_configuration_update(
            &mut record,
            AgentConfigurationUpdate {
                model: None,
                reset_model_to_template: false,
                provider: None,
                reset_provider_to_template: false,
                system_prompt: Some(Some("must not stick".into())),
                reset_system_prompt_to_template: false,
                skills: Some(vec![invalid]),
                reset_skills_to_template: false,
            },
        )
        .expect_err("unsafe path must fail");

        assert!(error.contains("safe relative path"));
        assert!(record.system_prompt_override.is_none());
        assert!(record.skill_overrides.is_none());
    }

    #[test]
    fn contradictory_reset_and_override_is_rejected() {
        let mut record = linked_record();
        assert!(apply_agent_configuration_update(
            &mut record,
            AgentConfigurationUpdate {
                model: Some(Some("agent-model".into())),
                reset_model_to_template: true,
                provider: None,
                reset_provider_to_template: false,
                system_prompt: None,
                reset_system_prompt_to_template: false,
                skills: None,
                reset_skills_to_template: false,
            },
        )
        .is_err());
        assert!(apply_agent_configuration_update(
            &mut record,
            AgentConfigurationUpdate {
                model: None,
                reset_model_to_template: false,
                provider: Some(Some("agent-provider".into())),
                reset_provider_to_template: true,
                system_prompt: None,
                reset_system_prompt_to_template: false,
                skills: None,
                reset_skills_to_template: false,
            },
        )
        .is_err());
        assert!(apply_agent_configuration_update(
            &mut record,
            AgentConfigurationUpdate {
                model: None,
                reset_model_to_template: false,
                provider: None,
                reset_provider_to_template: false,
                system_prompt: Some(Some("override".into())),
                reset_system_prompt_to_template: true,
                skills: None,
                reset_skills_to_template: false,
            },
        )
        .is_err());
        assert!(apply_agent_configuration_update(
            &mut record,
            AgentConfigurationUpdate {
                model: None,
                reset_model_to_template: false,
                provider: None,
                reset_provider_to_template: false,
                system_prompt: None,
                reset_system_prompt_to_template: false,
                skills: Some(vec![valid_skill("agent-skill")]),
                reset_skills_to_template: true,
            },
        )
        .is_err());
    }
}
