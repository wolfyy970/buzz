use crate::managed_agents::types::{AgentDefinition, ManagedAgentRecord};

/// Classify a linked agent against the live template catalog.
///
/// Returns `(out_of_date, orphaned)`. A hand-built agent has neither state,
/// while a missing linked template is orphaned but never described as stale.
pub(super) fn persona_drift_state(
    record: &ManagedAgentRecord,
    personas: &[AgentDefinition],
) -> (bool, bool) {
    let Some(persona_id) = record.persona_id.as_deref() else {
        return (false, false);
    };
    let Some(persona) = personas.iter().find(|persona| persona.id == persona_id) else {
        return (false, true);
    };
    let current = if record
        .persona_source_version
        .as_deref()
        .is_some_and(|version| version.starts_with("git:"))
    {
        persona
            .published_version
            .as_ref()
            .and_then(|version| version.authority_token().ok())
    } else {
        Some(crate::managed_agents::persona_events::persona_snapshot_version(persona))
    };
    let out_of_date = record
        .persona_source_version
        .as_deref()
        .is_some_and(|pinned| current.as_deref() != Some(pinned));
    (out_of_date, false)
}

/// An orphaned instance cannot restart successfully, so never offer a restart
/// action even when its saved process hash or adapter availability has drifted.
pub(super) fn restart_eligible(
    persona_orphaned: bool,
    hash_drift: bool,
    availability_drift: bool,
) -> bool {
    !persona_orphaned && (hash_drift || availability_drift)
}
