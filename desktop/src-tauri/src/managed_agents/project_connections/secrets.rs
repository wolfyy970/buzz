use std::collections::BTreeMap;
#[cfg(not(feature = "system-keyring"))]
use std::fs;

use tauri::AppHandle;

use super::ProjectConnection;
#[cfg(not(feature = "system-keyring"))]
use crate::managed_agents::{atomic_write_json_restricted, managed_agents_base_dir};
#[cfg(feature = "system-keyring")]
use crate::{app_state::keyring_service, secret_store::SecretStore};

#[cfg(feature = "system-keyring")]
fn connection_secret_key(id: &str, credential_generation: u64) -> String {
    format!("project-connection:{id}:{credential_generation}")
}

#[cfg(feature = "system-keyring")]
fn secret_store() -> &'static SecretStore {
    SecretStore::shared(keyring_service())
}

#[cfg(not(feature = "system-keyring"))]
fn connection_secret_path(
    app: &AppHandle,
    id: &str,
    credential_generation: u64,
) -> Result<std::path::PathBuf, String> {
    let dir = managed_agents_base_dir(app)?.join("project-connection-secrets");
    match fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("Buzz refused an unsafe connection credentials directory.".to_string());
        }
        Ok(metadata) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err(
                        "Connection credentials are not owner-only. Fix their permissions before continuing."
                            .to_string(),
                    );
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&dir)
                .map_err(|error| format!("failed to create connection credentials: {error}"))?;
            #[cfg(unix)]
            fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
                .map_err(|error| {
                    format!("failed to protect the connection credentials directory: {error}")
                })?;
        }
        Err(error) => {
            return Err(format!(
                "failed to inspect the connection credentials directory: {error}"
            ));
        }
    }
    Ok(dir.join(format!("{id}-{credential_generation}.json")))
}

#[cfg(not(feature = "system-keyring"))]
fn reject_unsafe_connection_secret(path: &std::path::Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to inspect saved connection credentials: {error}"
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Buzz refused an unsafe connection credentials path.".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "Connection credentials are not owner-only. Fix their permissions before continuing."
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn serialize_secrets(env: &BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    serde_json::to_vec(env)
        .map_err(|error| format!("failed to prepare connection credentials: {error}"))
}

pub(super) fn load_secrets(
    app: &AppHandle,
    connection: &ProjectConnection,
) -> Result<BTreeMap<String, String>, String> {
    if connection.env_keys.is_empty() {
        return Ok(BTreeMap::new());
    }
    #[cfg(feature = "system-keyring")]
    let _ = app;
    #[cfg(feature = "system-keyring")]
    let raw = secret_store()
        .load(&connection_secret_key(
            &connection.id,
            connection.credential_generation,
        ))
        .map_err(|_| {
            format!(
                "Sign in again to '{}'. Buzz could not read its saved credentials.",
                connection.name
            )
        })?
        .ok_or_else(|| {
            format!(
                "Sign in again to '{}'. Its saved credentials are missing.",
                connection.name
            )
        })?
        .into_bytes();
    #[cfg(not(feature = "system-keyring"))]
    let raw = {
        let path = connection_secret_path(app, &connection.id, connection.credential_generation)?;
        reject_unsafe_connection_secret(&path)?;
        fs::read(path).map_err(|_| {
            format!(
                "Sign in again to '{}'. Its saved credentials are missing.",
                connection.name
            )
        })?
    };
    let env: BTreeMap<String, String> = serde_json::from_slice(&raw).map_err(|_| {
        format!(
            "Sign in again to '{}'. Its credentials are invalid.",
            connection.name
        )
    })?;
    let actual: std::collections::BTreeSet<&str> = env.keys().map(String::as_str).collect();
    let expected: std::collections::BTreeSet<&str> =
        connection.env_keys.iter().map(String::as_str).collect();
    if actual != expected {
        return Err(format!(
            "Sign in again to '{}'. Its saved credentials are incomplete.",
            connection.name
        ));
    }
    Ok(env)
}

pub(super) fn store_secrets(
    app: &AppHandle,
    id: &str,
    credential_generation: u64,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    if env.is_empty() {
        return delete_secrets(app, id, credential_generation);
    }
    let serialized = serialize_secrets(env)?;
    #[cfg(feature = "system-keyring")]
    {
        let raw = String::from_utf8(serialized)
            .map_err(|_| "Buzz could not prepare these credentials.".to_string())?;
        let key = connection_secret_key(id, credential_generation);
        let store = secret_store();
        store.store(&key, &raw).map_err(|_| {
            "Buzz could not save these credentials in the system keyring.".to_string()
        })?;
        if !store
            .verify_stored_raw(&key, &raw)
            .map_err(|_| "Buzz could not verify the saved credentials.".to_string())?
        {
            return Err("Buzz could not verify the saved credentials.".to_string());
        }
    }
    #[cfg(not(feature = "system-keyring"))]
    {
        let path = connection_secret_path(app, id, credential_generation)?;
        reject_unsafe_connection_secret(&path)?;
        atomic_write_json_restricted(&path, &serialized)?;
    }
    Ok(())
}

pub(super) fn delete_secrets(
    app: &AppHandle,
    id: &str,
    credential_generation: u64,
) -> Result<(), String> {
    #[cfg(feature = "system-keyring")]
    {
        let _ = app;
        secret_store().delete(&connection_secret_key(id, credential_generation))
    }
    #[cfg(not(feature = "system-keyring"))]
    {
        let path = connection_secret_path(app, id, credential_generation)?;
        reject_unsafe_connection_secret(&path)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("failed to remove saved credentials: {error}")),
        }
    }
}
