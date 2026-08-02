//! Immutable Buzz-hosted Git identity for an agent template version.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub(crate) fn template_artifact_path_component(template_id: &str) -> Result<String, String> {
    if template_id.is_empty()
        || template_id.len() > 512
        || template_id.chars().any(char::is_control)
    {
        return Err("Template id is invalid.".to_string());
    }
    if template_id.len() <= 64
        && template_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Ok(template_id.to_string())
    } else {
        Ok(hex::encode(Sha256::digest(template_id.as_bytes())))
    }
}

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

    /// Restore a version reference from its stable local authority token.
    pub fn from_authority_token(value: &str) -> Result<Self, String> {
        let value = value
            .strip_prefix("git:")
            .ok_or_else(|| "Template version authority token is invalid.".to_string())?;
        let mut parts = value.rsplitn(4, ':');
        let artifact_sha256 = parts.next();
        let artifact_path = parts.next();
        let commit_oid = parts.next();
        let repo_address = parts.next();
        let version = Self {
            repo_address: repo_address.unwrap_or_default().to_string(),
            commit_oid: commit_oid.unwrap_or_default().to_string(),
            artifact_path: artifact_path.unwrap_or_default().to_string(),
            artifact_sha256: artifact_sha256.unwrap_or_default().to_string(),
        };
        version.validate()?;
        Ok(version)
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
    use super::{template_artifact_path_component, AgentTemplateVersionRef};

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
        let value = version();
        let token = value.authority_token().expect("authority token");
        assert!(token.starts_with("git:"));
        assert_eq!(
            AgentTemplateVersionRef::from_authority_token(&token).unwrap(),
            value
        );
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

    #[test]
    fn builtin_template_ids_use_a_stable_safe_path_component() {
        let component = template_artifact_path_component("builtin:fizz").unwrap();
        assert_eq!(component.len(), 64);
        assert!(component.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(
            component,
            template_artifact_path_component("builtin:fizz").unwrap()
        );
    }
}
