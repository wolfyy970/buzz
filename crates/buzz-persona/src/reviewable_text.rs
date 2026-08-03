//! Validation for instruction text that must be reviewed exactly as executed.

/// Reject control and formatting characters that can conceal instruction bytes.
pub fn validate_reviewable_text(
    value: &str,
    label: &str,
    allow_layout_controls: bool,
) -> Result<(), String> {
    for character in value.chars() {
        let allowed_layout_control = allow_layout_controls && matches!(character, '\n' | '\t');
        if (!allowed_layout_control && character.is_control())
            || is_unicode_line_or_paragraph_separator(character)
            || is_unicode_format(character)
            || is_default_ignorable(character)
        {
            return Err(format!(
                "{label} contains prohibited invisible or formatting character U+{:04X}",
                character as u32
            ));
        }
    }
    Ok(())
}

/// Unicode line and paragraph separators (`Zl` and `Zp`).
fn is_unicode_line_or_paragraph_separator(character: char) -> bool {
    matches!(character, '\u{2028}' | '\u{2029}')
}

/// Unicode general category `Cf` ranges from DerivedGeneralCategory 17.0.0.
fn is_unicode_format(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// Unicode `Default_Ignorable_Code_Point` ranges from DerivedCoreProperties.
fn is_default_ignorable(character: char) -> bool {
    matches!(
        character as u32,
        0x00AD
            | 0x034F
            | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFF8
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_visible_text_and_requested_layout_controls() {
        assert!(validate_reviewable_text(
            "Review changes.\n\tCall out security risks.",
            "Instructions",
            true,
        )
        .is_ok());
    }

    #[test]
    fn rejects_default_ignorable_and_non_layout_control_characters() {
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
            '\u{2028}',
            '\u{2029}',
            '\u{0600}',
            '\u{13430}',
            '\u{E007F}',
            '\0',
            '\r',
            '\u{0007}',
            '\u{0085}',
        ] {
            let value = format!("Review{character}changes");
            assert!(validate_reviewable_text(&value, "Instructions", true).is_err());
        }
    }
}
