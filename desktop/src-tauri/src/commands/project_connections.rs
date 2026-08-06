use tauri::{AppHandle, Manager as _};

use crate::managed_agents::project_connections::{
    self, CreateProjectConnectionRequest, ProjectConnection, ProjectConnectionScope,
    UpdateProjectConnectionRequest,
};

#[tauri::command]
pub async fn list_project_connections(
    app: AppHandle,
    project_scope: ProjectConnectionScope,
) -> Result<Vec<ProjectConnection>, String> {
    let state = app.state::<crate::app_state::AppState>();
    let captured_scope = {
        let _transition_guard = state.workspace_transition.lock().await;
        project_connections::capture_project_scope_for_app(&app, &project_scope)?
    };
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        project_connections::list_project_connections_at(&worker_app, &captured_scope)
    })
    .await
    .map_err(|error| format!("Project connection list task failed: {error}"))?
}

#[tauri::command]
pub async fn create_project_connection(
    app: AppHandle,
    input: CreateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    let state = app.state::<crate::app_state::AppState>();
    let captured_scope = {
        let _transition_guard = state.workspace_transition.lock().await;
        project_connections::capture_project_scope_for_app(&app, &input.project_scope)?
    };
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        project_connections::create_project_connection_at(&worker_app, &captured_scope, input)
    })
    .await
    .map_err(|error| format!("Project connection create task failed: {error}"))?
}

#[tauri::command]
pub async fn update_project_connection(
    app: AppHandle,
    input: UpdateProjectConnectionRequest,
) -> Result<ProjectConnection, String> {
    let state = app.state::<crate::app_state::AppState>();
    let captured_scope = {
        let _transition_guard = state.workspace_transition.lock().await;
        project_connections::capture_project_scope_for_app(&app, &input.project_scope)?
    };
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        project_connections::update_project_connection_at(&worker_app, &captured_scope, input)
    })
    .await
    .map_err(|error| format!("Project connection update task failed: {error}"))?
}

#[tauri::command]
pub async fn test_project_connection(
    app: AppHandle,
    project_scope: ProjectConnectionScope,
    connection_id: String,
) -> Result<ProjectConnection, String> {
    let state = app.state::<crate::app_state::AppState>();
    let captured_scope = {
        let _transition_guard = state.workspace_transition.lock().await;
        project_connections::capture_project_scope_for_app(&app, &project_scope)?
    };
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        project_connections::test_project_connection_at(
            &worker_app,
            &captured_scope,
            &connection_id,
        )
    })
    .await
    .map_err(|error| format!("Project connection test task failed: {error}"))?
}

#[tauri::command]
pub async fn delete_project_connection(
    app: AppHandle,
    project_scope: ProjectConnectionScope,
    connection_id: String,
) -> Result<(), String> {
    let state = app.state::<crate::app_state::AppState>();
    let captured_scope = {
        let _transition_guard = state.workspace_transition.lock().await;
        project_connections::capture_project_scope_for_app(&app, &project_scope)?
    };
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        project_connections::delete_project_connection_at(
            &worker_app,
            &captured_scope,
            &connection_id,
        )
    })
    .await
    .map_err(|error| format!("Project connection delete task failed: {error}"))?
}
