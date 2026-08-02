use std::collections::HashSet;

use crate::managed_agents::{AgentModelInfo, AgentModelsResponse};

/// Normalize raw `buzz-acp models --json` output into a typed DTO for the frontend.
///
/// Merges models from both ACP paths (stable configOptions + unstable
/// SessionModelState), deduplicates by ID, and returns a unified list.
pub(in crate::commands) fn normalize_agent_models(
    raw: &serde_json::Value,
    persisted_model: Option<String>,
) -> AgentModelsResponse {
    let agent_name = raw["agent"]["name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let agent_version = raw["agent"]["version"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    let mut models: Vec<AgentModelInfo> = Vec::new();
    let mut seen_ids = HashSet::new();

    if let Some(config_options) = raw["stable"]["configOptions"].as_array() {
        for opt in config_options {
            if opt.get("category").and_then(|c| c.as_str()) != Some("model") {
                continue;
            }
            if let Some(options) = opt.get("options").and_then(|v| v.as_array()) {
                for option in options {
                    if let Some(value) = option.get("value").and_then(|v| v.as_str()) {
                        if seen_ids.insert(value.to_string()) {
                            models.push(AgentModelInfo {
                                id: value.to_string(),
                                name: option
                                    .get("displayName")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string),
                                description: None,
                            });
                        }
                    }
                }
            }
        }
    }

    let mut agent_default_model = None;
    if let Some(unstable) = raw.get("unstable") {
        agent_default_model = unstable["currentModelId"].as_str().map(str::to_string);
        if let Some(available) = unstable["availableModels"].as_array() {
            for model in available {
                if let Some(id) = model.get("modelId").and_then(|v| v.as_str()) {
                    if seen_ids.insert(id.to_string()) {
                        models.push(AgentModelInfo {
                            id: id.to_string(),
                            name: model
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                            description: model
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                        });
                    }
                }
            }
        }
    }

    AgentModelsResponse {
        agent_name,
        agent_version,
        supports_switching: !models.is_empty(),
        models,
        agent_default_model,
        selected_model: persisted_model,
    }
}
