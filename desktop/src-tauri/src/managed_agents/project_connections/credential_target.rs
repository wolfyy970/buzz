use super::*;

/// Exact credential location authorized while a Project connection scope was
/// active.
///
/// Compensation may use this target after the generation lease is cancelled,
/// but it cannot derive a different workspace, Project, connection, or
/// credential generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CapturedCredentialTarget {
    pub(super) workspace_scope_id: String,
    pub(super) project: ProjectConnectionScope,
    pub(super) connection_id: String,
    pub(super) credential_generation: String,
    #[cfg(feature = "system-keyring")]
    pub(super) key: String,
    #[cfg(not(feature = "system-keyring"))]
    pub(super) path: PathBuf,
}

pub(super) fn capture_credential_target<R: tauri::Runtime>(
    app: &AppHandle<R>,
    scope: &CapturedProjectConnectionScope,
    connection_id: &str,
    credential_generation: &str,
) -> Result<CapturedCredentialTarget, String> {
    validate_captured_scope_for_app(app, scope)?;
    if !is_lower_hex(connection_id, 32) || !is_lower_hex(credential_generation, 32) {
        return Err("Project connection credential identity is invalid.".to_string());
    }

    #[cfg(feature = "system-keyring")]
    let key = format!(
        "project-connection:{}:{connection_id}:{credential_generation}",
        scope.workspace.scope_id
    );
    #[cfg(not(feature = "system-keyring"))]
    let path = {
        let dir = workspace_connection_dir(scope)?.join("secrets");
        ensure_owner_only_directory(&dir)?;
        let digest = Sha256::digest(format!("{connection_id}\0{credential_generation}").as_bytes());
        dir.join(format!("{}.json", hex::encode(digest)))
    };

    Ok(CapturedCredentialTarget {
        workspace_scope_id: scope.workspace.scope_id.clone(),
        project: scope.project.clone(),
        connection_id: connection_id.to_string(),
        credential_generation: credential_generation.to_string(),
        #[cfg(feature = "system-keyring")]
        key,
        #[cfg(not(feature = "system-keyring"))]
        path,
    })
}

#[cfg(test)]
pub(super) fn test_credential_target(
    workspace_scope_id: &str,
    project: ProjectConnectionScope,
    connection_id: &str,
    credential_generation: &str,
) -> CapturedCredentialTarget {
    CapturedCredentialTarget {
        workspace_scope_id: workspace_scope_id.to_string(),
        project,
        connection_id: connection_id.to_string(),
        credential_generation: credential_generation.to_string(),
        #[cfg(feature = "system-keyring")]
        key: format!(
            "project-connection:{workspace_scope_id}:{connection_id}:{credential_generation}"
        ),
        #[cfg(not(feature = "system-keyring"))]
        path: PathBuf::from(format!(
            "/test/{workspace_scope_id}/{connection_id}/{credential_generation}"
        )),
    }
}
