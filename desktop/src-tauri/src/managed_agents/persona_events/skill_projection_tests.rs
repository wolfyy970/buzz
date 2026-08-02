use super::*;
use crate::managed_agents::{AgentSkill, AgentSkillFile};

fn skill(body: &str) -> AgentSkill {
    let name = "campaign-analysis";
    let description = "Analyze campaign performance";
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
fn skill_content_is_publicly_projected_and_versions_the_template() {
    let mut persona =
        crate::managed_agents::personas::built_in_persona_definition("builtin:fizz", "now")
            .expect("built-in template");
    let original_hash = persona_content_hash(&persona_event_content(&persona));
    persona.skills = vec![skill("Use the weekly report.")];

    let content = persona_event_content(&persona);
    assert_eq!(content.skills, persona.skills);
    assert_ne!(persona_content_hash(&content), original_hash);

    let event = build_persona_event(&persona)
        .expect("template event")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign template event");
    let decoded = persona_from_event(&event).expect("decode template event");
    assert_eq!(decoded.skills, persona.skills);
}

#[test]
fn editing_a_skill_file_changes_the_template_version() {
    let mut persona =
        crate::managed_agents::personas::built_in_persona_definition("builtin:fizz", "now")
            .expect("built-in template");
    persona.skills = vec![skill("First workflow")];
    let first = persona_snapshot_version(&persona);
    persona.skills[0].files[0].content = skill("Second workflow").files[0].content.clone();
    let second = persona_snapshot_version(&persona);
    assert_ne!(first, second);
}
