use super::*;

fn skill(name: &str, description: &str, body: &str) -> PortableSkill {
    PortableSkill {
        name: name.to_string(),
        description: description.to_string(),
        files: vec![PortableSkillFile {
            path: "SKILL.md".to_string(),
            content: format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
        }],
    }
}

fn bundle(skills: Vec<PortableSkill>) -> SkillBundle {
    SkillBundle {
        schema_version: SKILL_BUNDLE_SCHEMA_VERSION,
        skills,
    }
}

#[test]
fn accepts_a_reviewable_common_bundle() {
    let candidate = bundle(vec![skill(
        "campaign-analysis",
        "Analyze campaign performance",
        "# Workflow\n\nCompare the approved metrics.",
    )]);
    assert!(candidate.validate().is_ok());
    assert!(candidate.plaintext_warnings().is_empty());
}

#[test]
fn rejects_empty_or_unknown_version_bundles() {
    assert!(bundle(Vec::new()).validate().is_err());

    let mut unknown = bundle(vec![skill("safe-skill", "Safe Skill", "Body")]);
    unknown.schema_version += 1;
    assert!(unknown.validate().is_err());
}

#[test]
fn canonical_hash_ignores_skill_and_file_order() {
    let mut alpha = skill("alpha", "Alpha Skill", "Alpha");
    alpha.files.push(PortableSkillFile {
        path: "references/metrics.md".to_string(),
        content: "Metrics".to_string(),
    });
    let beta = skill("beta", "Beta Skill", "Beta");

    let mut reordered_alpha = alpha.clone();
    reordered_alpha.files.reverse();
    assert_eq!(
        bundle(vec![alpha, beta.clone()]).canonical_hash().unwrap(),
        bundle(vec![beta, reordered_alpha])
            .canonical_hash()
            .unwrap()
    );
}

#[test]
fn rejects_traversal_absolute_backslash_hidden_and_non_ascii_paths() {
    for path in [
        "../secret",
        "/tmp/secret",
        "refs\\secret",
        ".hidden",
        "référence.md",
        "refs//secret",
    ] {
        let mut candidate = skill("safe-skill", "Safe Skill", "Body");
        candidate.files.push(PortableSkillFile {
            path: path.to_string(),
            content: "text".to_string(),
        });
        assert!(bundle(vec![candidate]).validate().is_err(), "{path}");
    }
}

#[test]
fn rejects_case_folded_path_collisions() {
    let mut candidate = skill("safe-skill", "Safe Skill", "Body");
    candidate.files.push(PortableSkillFile {
        path: "references/README.md".to_string(),
        content: "First".to_string(),
    });
    candidate.files.push(PortableSkillFile {
        path: "references/readme.md".to_string(),
        content: "Second".to_string(),
    });
    assert!(bundle(vec![candidate]).validate().is_err());
}

#[test]
fn rejects_file_and_directory_path_collisions() {
    let mut candidate = skill("safe-skill", "Safe Skill", "Body");
    candidate.files.push(PortableSkillFile {
        path: "references".to_string(),
        content: "A file".to_string(),
    });
    candidate.files.push(PortableSkillFile {
        path: "references/readme.md".to_string(),
        content: "A nested file".to_string(),
    });
    assert!(bundle(vec![candidate]).validate().is_err());
}

#[test]
fn rejects_windows_reserved_names_and_trailing_dots() {
    for path in ["CON", "aux.md", "refs/LPT1.txt", "trailing."] {
        let mut candidate = skill("safe-skill", "Safe Skill", "Body");
        candidate.files.push(PortableSkillFile {
            path: path.to_string(),
            content: "text".to_string(),
        });
        assert!(bundle(vec![candidate]).validate().is_err(), "{path}");
    }
}

#[test]
fn rejects_unknown_or_mismatched_frontmatter() {
    let mut unknown = skill("safe-skill", "Safe Skill", "Body");
    unknown.files[0].content =
        "---\nname: safe-skill\ndescription: Safe Skill\nallowed-tools: shell\n---\nBody"
            .to_string();
    assert!(bundle(vec![unknown]).validate().is_err());

    let mut mismatch = skill("safe-skill", "Safe Skill", "Body");
    mismatch.files[0].content = "---\nname: other\ndescription: Safe Skill\n---\nBody".to_string();
    assert!(bundle(vec![mismatch]).validate().is_err());
}

#[test]
fn rejects_multiline_or_padded_descriptions() {
    for description in [" Padded", "Padded ", "First line\nSecond line"] {
        let candidate = skill("safe-skill", description, "Body");
        assert!(
            bundle(vec![candidate]).validate().is_err(),
            "{description:?}"
        );
    }
}

#[test]
fn rejects_invisible_instruction_text() {
    let candidate = skill(
        "unsafe-skill",
        "Unsafe Skill",
        "Review this\u{2066} hidden direction",
    );
    assert!(bundle(vec![candidate]).validate().is_err());
}

#[test]
fn serialized_limit_accounts_for_json_escaping() {
    let quoted = "\"".repeat(MAX_SKILL_FILE_BYTES - 128);
    let mut candidate = skill("large-skill", "Large Skill", "Body");
    for index in 0..4 {
        candidate.files.push(PortableSkillFile {
            path: format!("references/part-{index}.md"),
            content: quoted.clone(),
        });
    }
    let error = bundle(vec![candidate]).validate().unwrap_err();
    assert!(error.contains("serialized Skill bundle"));
}

#[test]
fn plaintext_secret_scan_is_advisory_and_catches_common_shapes() {
    let risky = skill(
        "database-access",
        "Database access",
        "DATABASE_URL=postgres://user:real-password@host/database",
    );
    let candidate = bundle(vec![risky]);
    assert!(candidate.validate().is_ok());
    assert_eq!(candidate.plaintext_warnings().len(), 1);

    let placeholder = bundle(vec![skill(
        "safe-skill",
        "Safe Skill",
        "API_KEY=your-placeholder-key",
    )]);
    assert!(placeholder.validate().is_ok());
    assert!(placeholder.plaintext_warnings().is_empty());
}
