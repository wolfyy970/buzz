use super::*;

fn skill(name: &str, description: &str, body: &str) -> AgentSkill {
    AgentSkill {
        name: name.to_string(),
        description: description.to_string(),
        files: vec![AgentSkillFile {
            path: "SKILL.md".to_string(),
            content: format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
        }],
    }
}

#[test]
fn accepts_a_small_portable_skill() {
    assert!(validate_agent_skills(&[skill(
        "campaign-analysis",
        "Analyze campaign performance",
        "# Workflow"
    )])
    .is_ok());
}

#[test]
fn rejects_traversal_absolute_backslash_and_duplicate_paths() {
    for path in ["../secret", "/tmp/secret", "refs\\secret", "refs//secret"] {
        let mut candidate = skill("safe-skill", "Safe skill", "Body");
        candidate.files.push(AgentSkillFile {
            path: path.to_string(),
            content: "text".to_string(),
        });
        assert!(
            validate_agent_skills(&[candidate]).is_err(),
            "{path} should fail"
        );
    }

    let mut duplicate = skill("safe-skill", "Safe skill", "Body");
    duplicate.files.push(duplicate.files[0].clone());
    assert!(validate_agent_skills(&[duplicate]).is_err());
}

#[test]
fn rejects_missing_or_mismatched_skill_frontmatter() {
    let mut missing = skill("safe-skill", "Safe skill", "Body");
    missing.files[0].content = "# No frontmatter".to_string();
    assert!(validate_agent_skills(&[missing]).is_err());

    let mut mismatch = skill("safe-skill", "Different description", "Body");
    mismatch.files[0].content =
        "---\nname: other\ndescription: Different description\n---\n".to_string();
    assert!(validate_agent_skills(&[mismatch]).is_err());
}

#[test]
fn rejects_obvious_secrets_but_allows_placeholders() {
    let secret = skill(
        "unsafe-skill",
        "Unsafe skill",
        "OPENAI_API_KEY=sk-live-this-is-a-real-looking-secret",
    );
    assert!(validate_agent_skills(&[secret]).is_err());

    let placeholder = skill(
        "safe-skill",
        "Safe skill",
        "OPENAI_API_KEY=sk-your-key-here",
    );
    assert!(validate_agent_skills(&[placeholder]).is_ok());
}

#[test]
fn bundle_hash_is_order_independent() {
    let first = skill("alpha", "Alpha skill", "Alpha");
    let second = skill("beta", "Beta skill", "Beta");
    assert_eq!(
        skill_bundle_hash(&[first.clone(), second.clone()]).unwrap(),
        skill_bundle_hash(&[second, first]).unwrap()
    );
}

#[test]
fn existing_content_tree_must_match_its_hash() {
    let store = tempfile::tempdir().unwrap();
    let skills = vec![skill("alpha", "Alpha skill", "Alpha")];
    let target = store.path().join(skill_bundle_hash(&skills).unwrap());
    materialize_content_tree(store.path(), &target, &skills).unwrap();
    fs::write(target.join("alpha/SKILL.md"), "tampered").unwrap();

    let error = materialize_content_tree(store.path(), &target, &skills).unwrap_err();
    assert!(error.contains("does not match"));
}

#[cfg(unix)]
#[test]
fn copy_refuses_symlinks() {
    use std::os::unix::fs::symlink;

    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    symlink("/tmp", source.path().join("redirect")).unwrap();
    assert!(copy_owner_tree(source.path(), target.path()).is_err());
}

#[cfg(unix)]
#[test]
fn materialized_tree_is_owner_only_and_non_executable() {
    use std::os::unix::fs::PermissionsExt as _;

    let store = tempfile::tempdir().unwrap();
    let skills = vec![skill("alpha", "Alpha skill", "Alpha")];
    let target = store.path().join(skill_bundle_hash(&skills).unwrap());
    materialize_content_tree(store.path(), &target, &skills).unwrap();

    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(target.join("alpha/SKILL.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
