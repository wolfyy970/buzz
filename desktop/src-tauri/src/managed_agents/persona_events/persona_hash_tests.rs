use super::*;

fn content() -> PersonaEventContent {
    PersonaEventContent {
        published_version: None,
        display_name: "Test".to_string(),
        avatar_url: None,
        system_prompt: Some("Hello".to_string()),
        runtime: None,
        model: None,
        provider: None,
        name_pool: vec![],
        respond_to: None,
        respond_to_allowlist: Vec::new(),
        parallelism: None,
        tool_requirements: Vec::new(),
        skills: Vec::new(),
    }
}

#[test]
fn persona_content_hash_is_deterministic() {
    let content = content();
    let hash1 = persona_content_hash(&content);
    let hash2 = persona_content_hash(&content);
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 64);
}

#[test]
fn persona_content_hash_changes_on_edit() {
    let content1 = content();
    let mut content2 = content1.clone();
    content2.system_prompt = Some("Goodbye".to_string());
    assert_ne!(
        persona_content_hash(&content1),
        persona_content_hash(&content2)
    );
}
