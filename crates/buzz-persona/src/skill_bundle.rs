//! Runtime-neutral value model for portable, text-only agent Skills.
//!
//! This module deliberately stops at validation and canonical identity. It
//! does not publish, install, or activate a bundle. Those actions require an
//! exact-file review flow, immutable template versions, and an established
//! workspace boundary.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::reviewable_text::validate_reviewable_text;

pub(crate) const MAX_SKILLS_PER_BUNDLE: usize = 16;
pub(crate) const MAX_FILES_PER_SKILL: usize = 32;
pub(crate) const MAX_SKILL_FILE_BYTES: usize = 32 * 1024;
pub(crate) const MAX_SKILL_BUNDLE_BYTES: usize = 128 * 1024;
pub(crate) const MAX_SKILL_BUNDLE_SERIALIZED_BYTES: usize = 192 * 1024;
const MAX_SKILL_NAME_BYTES: usize = 64;
const MAX_SKILL_DESCRIPTION_BYTES: usize = 512;
const MAX_SKILL_PATH_BYTES: usize = 240;
const SKILL_BUNDLE_SCHEMA_VERSION: u16 = 1;

/// A validated collection of portable Skills.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillBundle {
    /// Schema version used to interpret and hash this bundle.
    pub schema_version: u16,
    /// Skills included in the bundle.
    pub skills: Vec<PortableSkill>,
}

/// One portable Skill and its complete text file tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PortableSkill {
    /// Stable lowercase Skill identifier.
    pub name: String,
    /// Short description shown before the Skill is loaded.
    pub description: String,
    /// Complete text files, including a root `SKILL.md`.
    pub files: Vec<PortableSkillFile>,
}

/// One text file inside a portable Skill.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PortableSkillFile {
    /// Portable POSIX-style path relative to the Skill root.
    pub path: String,
    /// Exact UTF-8 file content reviewed by the user.
    pub content: String,
}

/// Advisory finding that must be disclosed before a bundle is published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillBundleWarning {
    /// Skill containing the finding.
    pub skill_name: String,
    /// File containing the finding.
    pub path: String,
    /// Human-readable reason for the warning.
    pub message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommonSkillFrontmatter {
    name: String,
    description: String,
}

impl SkillBundle {
    /// Validate the complete bundle without publishing or materializing it.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SKILL_BUNDLE_SCHEMA_VERSION {
            return Err(format!(
                "Skill bundle schema version {} is unsupported.",
                self.schema_version
            ));
        }
        if self.skills.is_empty() || self.skills.len() > MAX_SKILLS_PER_BUNDLE {
            return Err(format!(
                "A bundle must contain 1 to {MAX_SKILLS_PER_BUNDLE} Skills."
            ));
        }

        let mut skill_names = HashSet::new();
        let mut total_bytes = 0usize;
        for skill in &self.skills {
            validate_skill_name(&skill.name)?;
            if !skill_names.insert(skill.name.as_str()) {
                return Err(format!("Skill {:?} appears more than once.", skill.name));
            }
            validate_skill_description(skill)?;
            validate_skill_files(skill, &mut total_bytes)?;
        }

        let serialized = serde_json::to_vec(self)
            .map_err(|error| format!("Could not serialize Skill bundle: {error}"))?;
        if serialized.len() > MAX_SKILL_BUNDLE_SERIALIZED_BYTES {
            return Err(format!(
                "The serialized Skill bundle exceeds {MAX_SKILL_BUNDLE_SERIALIZED_BYTES} bytes."
            ));
        }
        Ok(())
    }

    /// Return a stable SHA-256 identity for the validated bundle.
    pub fn canonical_hash(&self) -> Result<String, String> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical
            .skills
            .sort_by(|left, right| left.name.cmp(&right.name));
        for skill in &mut canonical.skills {
            skill.files.sort_by(|left, right| {
                portable_path_key(&left.path).cmp(&portable_path_key(&right.path))
            });
        }
        let serialized = serde_json::to_vec(&canonical)
            .map_err(|error| format!("Could not serialize Skill bundle: {error}"))?;
        Ok(hex::encode(Sha256::digest(serialized)))
    }

    /// Return best-effort secret warnings without claiming exhaustive detection.
    pub fn plaintext_warnings(&self) -> Vec<SkillBundleWarning> {
        self.skills
            .iter()
            .flat_map(|skill| {
                skill
                    .files
                    .iter()
                    .filter(|file| file_looks_like_it_contains_a_secret(&file.content))
                    .map(|file| {
                        SkillBundleWarning {
                            skill_name: skill.name.clone(),
                            path: file.path.clone(),
                            message: "This file may contain a credential. Skill files are plaintext and must be reviewed before publication.".to_string(),
                        }
                    })
            })
            .collect()
    }
}

fn validate_skill_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= MAX_SKILL_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && name
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Skill name {name:?} must be a lowercase slug using letters, numbers, and hyphens."
        ))
    }
}

fn validate_skill_description(skill: &PortableSkill) -> Result<(), String> {
    let description = skill.description.trim();
    if description.is_empty()
        || description != skill.description
        || skill.description.len() > MAX_SKILL_DESCRIPTION_BYTES
    {
        return Err(format!(
            "Skill {:?} needs a single-line description without surrounding whitespace, no longer than {MAX_SKILL_DESCRIPTION_BYTES} bytes.",
            skill.name
        ));
    }
    validate_reviewable_text(description, "Skill description", false)
        .map_err(|error| format!("Skill {:?} description is unsafe: {error}", skill.name))
}

fn validate_skill_files(skill: &PortableSkill, total_bytes: &mut usize) -> Result<(), String> {
    if skill.files.is_empty() || skill.files.len() > MAX_FILES_PER_SKILL {
        return Err(format!(
            "Skill {:?} must include 1 to {MAX_FILES_PER_SKILL} files.",
            skill.name
        ));
    }

    let mut portable_paths = HashSet::new();
    let mut root_skill_file = None;
    for file in &skill.files {
        validate_portable_skill_path(&file.path)?;
        let path_key = portable_path_key(&file.path);
        if portable_paths.iter().any(|existing: &String| {
            existing == &path_key
                || existing
                    .strip_prefix(&path_key)
                    .is_some_and(|suffix| suffix.starts_with('/'))
                || path_key
                    .strip_prefix(existing)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) {
            return Err(format!(
                "Skill {:?} contains paths that collide as files or directories on a supported filesystem.",
                skill.name
            ));
        }
        portable_paths.insert(path_key);
        if file.content.len() > MAX_SKILL_FILE_BYTES {
            return Err(format!(
                "{} in Skill {:?} exceeds {MAX_SKILL_FILE_BYTES} bytes.",
                file.path, skill.name
            ));
        }
        validate_reviewable_text(&file.content, "Skill file", true).map_err(|error| {
            format!(
                "{} in Skill {:?} contains unsafe instruction text: {error}",
                file.path, skill.name
            )
        })?;
        *total_bytes = total_bytes
            .checked_add(file.path.len())
            .and_then(|value| value.checked_add(file.content.len()))
            .ok_or_else(|| "Skill bundle size overflowed.".to_string())?;
        if *total_bytes > MAX_SKILL_BUNDLE_BYTES {
            return Err(format!(
                "Skill files can contain at most {MAX_SKILL_BUNDLE_BYTES} bytes in total."
            ));
        }
        if file.path == "SKILL.md" {
            root_skill_file = Some(file);
        }
    }

    let root_skill_file =
        root_skill_file.ok_or_else(|| format!("Skill {:?} is missing SKILL.md.", skill.name))?;
    validate_common_frontmatter(skill, &root_skill_file.content)
}

fn validate_portable_skill_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > MAX_SKILL_PATH_BYTES
        || !path.is_ascii()
        || path.contains('\\')
    {
        return Err(format!("{path:?} is not a portable Skill path."));
    }

    for segment in path.split('/') {
        let valid_shape = !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            && segment
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && segment
                .bytes()
                .last()
                .is_some_and(|byte| byte.is_ascii_alphanumeric());
        if !valid_shape || windows_reserved_segment(segment) {
            return Err(format!("{path:?} is not a portable Skill path."));
        }
    }
    Ok(())
}

fn portable_path_key(path: &str) -> String {
    path.to_ascii_lowercase()
}

fn windows_reserved_segment(segment: &str) -> bool {
    let stem = segment
        .split_once('.')
        .map_or(segment, |(candidate, _)| candidate)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn validate_common_frontmatter(skill: &PortableSkill, content: &str) -> Result<(), String> {
    let normalized = content.replace("\r\n", "\n");
    let remainder = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| format!("{} SKILL.md must start with YAML frontmatter.", skill.name))?;
    let closing = remainder
        .find("\n---\n")
        .ok_or_else(|| format!("{} SKILL.md has incomplete YAML frontmatter.", skill.name))?;
    let metadata: CommonSkillFrontmatter = serde_yaml::from_str(&remainder[..closing])
        .map_err(|error| format!("{} SKILL.md frontmatter is invalid: {error}", skill.name))?;
    if metadata.name != skill.name {
        return Err(format!(
            "{} SKILL.md name must exactly match the Skill name.",
            skill.name
        ));
    }
    if metadata.description != skill.description {
        return Err(format!(
            "{} SKILL.md description must exactly match the Skill description.",
            skill.name
        ));
    }
    Ok(())
}

fn file_looks_like_it_contains_a_secret(content: &str) -> bool {
    let lowercase = content.to_ascii_lowercase();
    if [
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "-----begin openssh private key-----",
        "authorization: bearer ",
        "database_url=",
        "database_url:",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker))
    {
        return true;
    }

    for line in content.lines() {
        let line = line.trim().trim_start_matches("export ").trim();
        let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) else {
            continue;
        };
        let key = key
            .trim_matches(|character: char| {
                character == '"' || character == '\'' || character.is_whitespace()
            })
            .to_ascii_uppercase();
        if ![
            "API_KEY",
            "TOKEN",
            "PASSWORD",
            "SECRET",
            "PRIVATE_KEY",
            "AUTHORIZATION",
            "DATABASE_URL",
        ]
        .iter()
        .any(|marker| key.contains(marker))
        {
            continue;
        }
        let value = value
            .trim()
            .trim_matches(|character| character == '"' || character == '\'');
        if value.len() >= 8 && !looks_like_placeholder(value) {
            return true;
        }
    }
    false
}

fn looks_like_placeholder(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    [
        "example",
        "placeholder",
        "your-",
        "your_",
        "replace",
        "xxxx",
        "<",
        "${",
        "test-only",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker))
}

#[cfg(test)]
#[path = "skill_bundle/tests.rs"]
mod tests;
