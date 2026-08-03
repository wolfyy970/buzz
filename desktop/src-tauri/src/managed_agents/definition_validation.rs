//! Validation for human-reviewed agent definition text.
//!
//! Shared definitions are executable configuration: `system_prompt` is shown
//! to a person, then delivered verbatim to an ACP harness. Characters that
//! consume input bytes without a visible glyph break that review invariant and
//! are rejected rather than silently stripped.

const MAX_DISPLAY_NAME_CHARS: usize = 128;
const MAX_SYSTEM_PROMPT_BYTES: usize = 64 * 1024;

/// Validate the human-visible fields of an agent definition.
pub(crate) fn validate_agent_definition_text(
    display_name: &str,
    system_prompt: &str,
) -> Result<(), String> {
    if display_name.trim().is_empty() {
        return Err("Display name is required".to_string());
    }
    let display_name_chars = display_name.chars().count();
    if display_name_chars > MAX_DISPLAY_NAME_CHARS {
        return Err(format!(
            "Display name is too long ({display_name_chars} characters, max {MAX_DISPLAY_NAME_CHARS})"
        ));
    }
    if system_prompt.len() > MAX_SYSTEM_PROMPT_BYTES {
        return Err(format!(
            "Agent instructions are too long ({} bytes, max {MAX_SYSTEM_PROMPT_BYTES})",
            system_prompt.len()
        ));
    }

    validate_visible_text(display_name, "Display name", false)?;
    validate_visible_text(system_prompt, "Agent instructions", true)
}

fn validate_visible_text(
    value: &str,
    label: &str,
    allow_layout_controls: bool,
) -> Result<(), String> {
    buzz_persona_pkg::reviewable_text::validate_reviewable_text(
        value,
        label,
        allow_layout_controls,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_multiline_instructions() {
        assert!(validate_agent_definition_text(
            "Code Reviewer 🐝",
            "Review changes.\n\tCall out security risks."
        )
        .is_ok());
    }

    #[test]
    fn rejects_default_ignorable_characters_in_name_or_prompt() {
        for character in [
            '\u{00AD}',
            '\u{034F}',
            '\u{200B}',
            '\u{200D}',
            '\u{202E}',
            '\u{2060}',
            '\u{2066}',
            '\u{3164}',
            '\u{FE0F}',
            '\u{E007F}',
        ] {
            let name = format!("Review{character}er");
            let prompt = format!("Review code.{character}");
            assert!(validate_agent_definition_text(&name, "Review code.").is_err());
            assert!(validate_agent_definition_text("Reviewer", &prompt).is_err());
        }
    }

    #[test]
    fn rejects_non_layout_control_characters() {
        for character in ['\0', '\r', '\u{0007}', '\u{0085}'] {
            let prompt = format!("Review{character}code");
            assert!(validate_agent_definition_text("Reviewer", &prompt).is_err());
        }
    }

    #[test]
    fn enforces_display_name_and_prompt_bounds() {
        assert!(validate_agent_definition_text(&"a".repeat(129), "prompt").is_err());
        assert!(validate_agent_definition_text("Reviewer", &"a".repeat(64 * 1024 + 1)).is_err());
    }
}
