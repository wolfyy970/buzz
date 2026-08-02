//! Immutable Buzz-hosted Git identity for an agent template version.

use serde::{Deserialize, Serialize};

/// Full authority tuple for one published agent template version.
///
/// A label or short hash is display-only. Loading and applying a version must
/// verify all four fields against the active Buzz-hosted Git repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateVersionRef {
    pub repo_address: String,
    pub commit_oid: String,
    pub artifact_path: String,
    pub artifact_sha256: String,
}

impl AgentTemplateVersionRef {
    /// Stable local token stored on managed-agent revision history.
    pub fn authority_token(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!(
            "git:{}:{}:{}:{}",
            self.repo_address, self.commit_oid, self.artifact_path, self.artifact_sha256
        ))
    }

    /// Reject malformed values before any field reaches Git as an argument.
    pub fn validate(&self) -> Result<(), String> {
        validate_repo_address(&self.repo_address)?;
        validate_hex(&self.commit_oid, &[40, 64], "Template version commit")?;
        validate_artifact_path(&self.artifact_path)?;
        validate_hex(
            &self.artifact_sha256,
            &[64],
            "Template version artifact digest",
        )
    }
}

fn validate_repo_address(value: &str) -> Result<(), String> {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next();
    let owner = parts.next();
    let repo_id = parts.next();
    if kind != Some("30617")
        || owner.is_none_or(|owner| {
            owner.len() != 64 || !owner.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || repo_id.is_none_or(|repo_id| repo_id != "buzz-agent-templates")
    {
        return Err("Template version repository address is invalid.".to_string());
    }
    Ok(())
}

fn validate_hex(value: &str, lengths: &[usize], label: &str) -> Result<(), String> {
    if lengths.contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{label} is invalid."))
    }
}

fn validate_artifact_path(value: &str) -> Result<(), String> {
    let parts: Vec<&str> = value.split('/').collect();
    let valid = parts.len() == 5
        && parts[0] == "templates"
        && is_template_id(parts[1])
        && parts[2] == "versions"
        && parts[3].len() == 64
        && parts[3].bytes().all(|byte| byte.is_ascii_hexdigit())
        && parts[4] == "template.json";
    if valid {
        Ok(())
    } else {
        Err("Template version artifact path is invalid.".to_string())
    }
}

fn is_template_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::AgentTemplateVersionRef;

    fn version() -> AgentTemplateVersionRef {
        AgentTemplateVersionRef {
            repo_address: format!("30617:{}:buzz-agent-templates", "a".repeat(64)),
            commit_oid: "b".repeat(40),
            artifact_path: format!(
                "templates/analytics/versions/{}/template.json",
                "c".repeat(64)
            ),
            artifact_sha256: "c".repeat(64),
        }
    }

    #[test]
    fn complete_authority_tuple_validates() {
        assert!(version().validate().is_ok());
        assert!(version()
            .authority_token()
            .expect("authority token")
            .starts_with("git:"));
    }

    #[test]
    fn malformed_fields_fail_before_git() {
        let mut value = version();
        value.commit_oid = "--upload-pack=evil".to_string();
        assert!(value.validate().is_err());

        let mut value = version();
        value.artifact_path = "../secret".to_string();
        assert!(value.validate().is_err());

        let mut value = version();
        value.repo_address = format!("30617:{}:foreign", "a".repeat(64));
        assert!(value.validate().is_err());
    }
}
