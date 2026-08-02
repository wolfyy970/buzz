//! Portable, non-executable skill content owned by an agent template.

use serde::{Deserialize, Serialize};

/// One UTF-8 file in a portable agent skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentSkillFile {
    /// Strict relative path inside the skill directory.
    pub path: String,
    /// UTF-8 text content. V1 deliberately does not carry binary files or mode
    /// bits, so imported files can never become executable.
    pub content: String,
}

/// One portable skill bundled with an agent template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentSkill {
    /// Lowercase template-local slug and runtime load key.
    pub name: String,
    /// Short description shown before the runtime loads the full skill.
    pub description: String,
    /// Complete skill tree, including exactly one root `SKILL.md`.
    pub files: Vec<AgentSkillFile>,
}
