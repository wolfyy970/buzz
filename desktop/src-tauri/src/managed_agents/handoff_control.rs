use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tauri::AppHandle;

use super::{managed_agents_base_dir, ManagedAgentRuntimeKey};

const CHECKPOINT_FILE: &str = "checkpoint.json";
const REQUEST_FILE: &str = "update.request.json";
const CHECKPOINT_MAX_BYTES: u64 = 256 * 1024;
const CHECKPOINT_MAX_FUTURE_SKEW_SECS: u64 = 300;
const CHECKPOINT_MAX_CHANNELS: usize = 256;
const CHECKPOINT_MAX_COMPLETED_IDS_PER_CHANNEL: usize = 512;
const CHECKPOINT_MAX_COMPLETED_IDS_TOTAL: usize = 2_048;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffCheckpointEnvelope {
    version: u32,
    kind: String,
    handoff_id: Option<String>,
    start_nonce: Option<String>,
    phase: Option<String>,
    agent_pubkey: String,
    relay_sha256: String,
    written_at_unix_secs: u64,
    membership_replay_from: u64,
    channels: Vec<HandoffChannelCheckpoint>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffChannelCheckpoint {
    channel_id: uuid::Uuid,
    replay_from: u64,
    completed_event_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedUpdateIdentity {
    pub(crate) handoff_id: String,
    pub(crate) start_nonce: String,
}

impl PlannedUpdateIdentity {
    pub(crate) fn new(start_nonce: &str) -> Result<Self, String> {
        validate_start_nonce(start_nonce)?;
        Ok(Self {
            handoff_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            start_nonce: start_nonce.to_string(),
        })
    }
}

#[derive(Serialize)]
struct PlannedUpdateRequest<'a> {
    version: u32,
    kind: &'static str,
    handoff_id: &'a str,
    expected_start_nonce: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishedPlannedUpdateRequest {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlannedUpdateRequestState {
    Pending,
    Accepted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedAgentHandoffPaths {
    pub(crate) checkpoint: PathBuf,
    pub(crate) request: PathBuf,
}

pub(crate) fn prepare_managed_agent_handoff(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
) -> Result<ManagedAgentHandoffPaths, String> {
    prepare_handoff_at(&managed_agents_base_dir(app)?, key)
}

fn prepare_handoff_at(
    agents_dir: &Path,
    key: &ManagedAgentRuntimeKey,
) -> Result<ManagedAgentHandoffPaths, String> {
    let root = agents_dir.join("handoff");
    ensure_private_directory(&root)?;
    let pair_dir = root.join(key.runtime_id());
    ensure_private_directory(&pair_dir)?;
    let paths = ManagedAgentHandoffPaths {
        checkpoint: pair_dir.join(CHECKPOINT_FILE),
        request: pair_dir.join(REQUEST_FILE),
    };
    remove_planned_update_request(&paths.request)?;
    Ok(paths)
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(format!(
                "Agent update state must be a private directory, not a link or file: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| {
                format!(
                    "Could not create private agent update state at {}: {error}",
                    path.display()
                )
            })?;
        }
        Err(error) => {
            return Err(format!(
                "Could not inspect agent update state at {}: {error}",
                path.display()
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let expected_uid = path
            .parent()
            .ok_or_else(|| {
                format!(
                    "Agent update state has no private parent: {}",
                    path.display()
                )
            })?
            .metadata()
            .map_err(|error| {
                format!(
                    "Could not verify the owner of agent update state at {}: {error}",
                    path.display()
                )
            })?
            .uid();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "Could not protect agent update state at {}: {error}",
                path.display()
            )
        })?;
        let metadata = fs::metadata(path).map_err(|error| {
            format!(
                "Could not verify agent update state at {}: {error}",
                path.display()
            )
        })?;
        if metadata.uid() != expected_uid {
            return Err(format!(
                "Agent update state is not owned by the current user: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn refuse_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "Agent update control file must not be a link: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Could not inspect agent update control file at {}: {error}",
            path.display()
        )),
    }
}

pub(crate) fn remove_planned_update_request(path: &Path) -> Result<(), String> {
    refuse_link(path)?;
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Could not clear the agent update request at {}: {error}",
            path.display()
        )),
    }
}

fn sync_parent(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or_else(|| {
            format!(
                "Agent update control file has no private parent: {}",
                path.display()
            )
        })?;
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                format!(
                    "Could not make the agent update state durable at {}: {error}",
                    parent.display()
                )
            })?;
    }
    Ok(())
}

fn validate_start_nonce(start_nonce: &str) -> Result<(), String> {
    if start_nonce.len() == 32
        && start_nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err("The running agent has an invalid process-generation identity.".to_string())
    }
}

fn request_bytes(identity: &PlannedUpdateIdentity) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&PlannedUpdateRequest {
        version: 2,
        kind: "buzz-acp-planned-update-request",
        handoff_id: &identity.handoff_id,
        expected_start_nonce: &identity.start_nonce,
    })
    .map_err(|error| format!("Could not encode the agent update request: {error}"))
}

pub(crate) fn write_planned_update_request(
    path: &Path,
    identity: &PlannedUpdateIdentity,
) -> Result<PublishedPlannedUpdateRequest, String> {
    refuse_link(path)?;
    let bytes = request_bytes(identity)?;
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Agent update request has no private parent: {}",
            path.display()
        )
    })?;
    ensure_private_directory(parent)?;
    let temporary = parent.join(format!(".update-request-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|error| {
            format!(
                "Could not create the agent update request at {}: {error}",
                temporary.display()
            )
        })?;
        file.write_all(&bytes).map_err(|error| {
            format!(
                "Could not write the agent update request at {}: {error}",
                temporary.display()
            )
        })?;
        file.sync_all().map_err(|error| {
            format!(
                "Could not make the agent update request durable at {}: {error}",
                temporary.display()
            )
        })?;
        fs::hard_link(&temporary, path).map_err(|error| {
            format!(
                "Could not publish the agent update request at {}: {error}",
                path.display()
            )
        })?;
        fs::remove_file(&temporary).map_err(|error| {
            format!(
                "Could not clear the temporary agent update request at {}: {error}",
                temporary.display()
            )
        })?;
        sync_parent(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|()| PublishedPlannedUpdateRequest { bytes })
}

pub(crate) fn planned_update_request_state(
    path: &Path,
    published: &PublishedPlannedUpdateRequest,
) -> Result<PlannedUpdateRequestState, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "The agent update request changed before it was accepted: {}",
            path.display()
        )),
        Ok(_) => {
            let current = fs::read(path).map_err(|error| {
                format!(
                    "Could not verify the agent update request at {}: {error}",
                    path.display()
                )
            })?;
            if current == published.bytes {
                Ok(PlannedUpdateRequestState::Pending)
            } else {
                Err(format!(
                    "The agent update request changed before it was accepted: {}",
                    path.display()
                ))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PlannedUpdateRequestState::Accepted)
        }
        Err(error) => Err(format!(
            "Could not inspect the agent update request at {}: {error}",
            path.display()
        )),
    }
}

pub(crate) fn cancel_planned_update_request(
    path: &Path,
    published: &PublishedPlannedUpdateRequest,
) -> Result<bool, String> {
    match planned_update_request_state(path, published)? {
        PlannedUpdateRequestState::Accepted => Ok(false),
        PlannedUpdateRequestState::Pending => {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => {
                    return Err(format!(
                        "Could not cancel the agent update request at {}: {error}",
                        path.display()
                    ));
                }
            }
            sync_parent(path)?;
            Ok(true)
        }
    }
}

pub(crate) fn verify_handoff_checkpoint(
    path: &Path,
    key: &ManagedAgentRuntimeKey,
    identity: &PlannedUpdateIdentity,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "The agent stopped without a durable update checkpoint at {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "The agent update checkpoint is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() == 0 || metadata.len() > CHECKPOINT_MAX_BYTES {
        return Err(format!(
            "The agent update checkpoint has an invalid size at {}.",
            path.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let expected_uid = path
            .parent()
            .ok_or_else(|| {
                format!(
                    "The agent update checkpoint has no private parent: {}",
                    path.display()
                )
            })?
            .metadata()
            .map_err(|error| {
                format!(
                    "Could not verify the owner of the agent update checkpoint at {}: {error}",
                    path.display()
                )
            })?
            .uid();
        if metadata.uid() != expected_uid || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(format!(
                "The agent update checkpoint is not private to the current user: {}",
                path.display()
            ));
        }
    }
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "Could not read the agent update checkpoint at {}: {error}",
            path.display()
        )
    })?;
    let document: HandoffCheckpointEnvelope = serde_json::from_slice(&bytes)
        .map_err(|error| format!("The agent update checkpoint is not valid: {error}"))?;
    let expected_relay = hex::encode(Sha256::digest(key.relay_url.as_bytes()));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let identity_is_valid =
        uuid::Uuid::parse_str(document.handoff_id.as_deref().unwrap_or_default())
            .ok()
            .is_some_and(|handoff_id| {
                handoff_id.hyphenated().to_string()
                    == document.handoff_id.as_deref().unwrap_or_default()
            })
            && document
                .start_nonce
                .as_deref()
                .is_some_and(|start_nonce| validate_start_nonce(start_nonce).is_ok());
    let relay_is_valid = document.relay_sha256.len() == 64
        && document
            .relay_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if document.version != 3
        || document.kind != "buzz-acp-planned-update"
        || !identity_is_valid
        || document.handoff_id.as_deref() != Some(identity.handoff_id.as_str())
        || document.start_nonce.as_deref() != Some(identity.start_nonce.as_str())
        || document.phase.as_deref() != Some("finalized")
        || document.agent_pubkey != key.pubkey
        || !relay_is_valid
        || document.relay_sha256 != expected_relay
        || document.written_at_unix_secs > now.saturating_add(CHECKPOINT_MAX_FUTURE_SKEW_SECS)
        || document.membership_replay_from > document.written_at_unix_secs
        || document.channels.len() > CHECKPOINT_MAX_CHANNELS
    {
        return Err(
            "The agent update checkpoint does not match this agent and community.".to_string(),
        );
    }
    let mut channel_ids = std::collections::HashSet::with_capacity(document.channels.len());
    let mut completed_ids = std::collections::HashSet::new();
    for channel in &document.channels {
        if !channel_ids.insert(channel.channel_id)
            || channel.replay_from > document.written_at_unix_secs
            || channel.completed_event_ids.len() > CHECKPOINT_MAX_COMPLETED_IDS_PER_CHANNEL
        {
            return Err("The agent update checkpoint contains invalid replay state.".to_string());
        }
        for event_id in &channel.completed_event_ids {
            if event_id.len() != 64
                || !event_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !completed_ids.insert(event_id)
            {
                return Err(
                    "The agent update checkpoint contains invalid completed work.".to_string(),
                );
            }
        }
    }
    if completed_ids.len() > CHECKPOINT_MAX_COMPLETED_IDS_TOTAL {
        return Err("The agent update checkpoint contains too much completed work.".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("buzz-desktop-handoff-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).expect("create temp");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn key() -> ManagedAgentRuntimeKey {
        ManagedAgentRuntimeKey::new("aa".repeat(32), "wss://relay.example").expect("key")
    }

    fn identity() -> PlannedUpdateIdentity {
        PlannedUpdateIdentity {
            handoff_id: "123e4567-e89b-12d3-a456-426614174000".to_string(),
            start_nonce: "12".repeat(16),
        }
    }

    #[test]
    fn pair_paths_are_stable_private_and_clear_stale_requests() {
        let temp = TempDir::new();
        let first = prepare_handoff_at(&temp.0, &key()).expect("paths");
        write_planned_update_request(&first.request, &identity()).expect("request");
        let second = prepare_handoff_at(&temp.0, &key()).expect("paths again");
        assert_eq!(first, second);
        assert!(!second.request.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(second.request.parent().expect("parent"))
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn request_is_exact_and_owner_only() {
        let temp = TempDir::new();
        let paths = prepare_handoff_at(&temp.0, &key()).expect("paths");
        let request = identity();
        write_planned_update_request(&paths.request, &request).expect("request");
        assert_eq!(
            fs::read(&paths.request).expect("read"),
            br#"{"version":2,"kind":"buzz-acp-planned-update-request","handoff_id":"123e4567-e89b-12d3-a456-426614174000","expected_start_nonce":"12121212121212121212121212121212"}"#
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&paths.request)
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn request_symlink_is_refused_without_touching_target() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let paths = prepare_handoff_at(&temp.0, &key()).expect("paths");
        let target = temp.0.join("target");
        fs::write(&target, b"keep").expect("target");
        symlink(&target, &paths.request).expect("symlink");
        assert!(write_planned_update_request(&paths.request, &identity()).is_err());
        assert_eq!(fs::read(target).expect("read target"), b"keep");
    }

    #[test]
    fn request_publish_never_replaces_an_existing_request() {
        let temp = TempDir::new();
        let paths = prepare_handoff_at(&temp.0, &key()).expect("paths");
        fs::write(&paths.request, b"keep").expect("existing");
        assert!(write_planned_update_request(&paths.request, &identity()).is_err());
        assert_eq!(fs::read(&paths.request).expect("read"), b"keep");
    }

    #[test]
    fn cancellation_only_removes_the_exact_published_request() {
        let temp = TempDir::new();
        let paths = prepare_handoff_at(&temp.0, &key()).expect("paths");
        let published = write_planned_update_request(&paths.request, &identity()).expect("request");
        fs::write(&paths.request, b"changed").expect("replace contents");
        assert!(cancel_planned_update_request(&paths.request, &published).is_err());
        assert_eq!(fs::read(&paths.request).expect("read"), b"changed");
    }

    #[test]
    fn cancellation_treats_an_already_consumed_request_as_accepted() {
        let temp = TempDir::new();
        let paths = prepare_handoff_at(&temp.0, &key()).expect("paths");
        let published = write_planned_update_request(&paths.request, &identity()).expect("request");
        fs::remove_file(&paths.request).expect("simulate ACP consumption");
        assert!(!cancel_planned_update_request(&paths.request, &published).expect("accepted"));
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_must_be_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = TempDir::new();
        let paths = prepare_handoff_at(&temp.0, &key()).expect("paths");
        fs::write(&paths.checkpoint, b"{}").expect("checkpoint");
        fs::set_permissions(&paths.checkpoint, fs::Permissions::from_mode(0o644))
            .expect("permissions");
        assert!(verify_handoff_checkpoint(&paths.checkpoint, &key(), &identity()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn handoff_root_symlink_is_refused() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let outside = temp.0.join("outside");
        fs::create_dir(&outside).expect("outside");
        symlink(&outside, temp.0.join("handoff")).expect("symlink");
        assert!(prepare_handoff_at(&temp.0, &key()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_must_match_the_exact_runtime_pair() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = TempDir::new();
        let runtime_key = key();
        let paths = prepare_handoff_at(&temp.0, &runtime_key).expect("paths");
        let relay_sha256 = hex::encode(Sha256::digest(runtime_key.relay_url.as_bytes()));
        let identity = identity();
        let document = serde_json::json!({
            "version": 3,
            "kind": "buzz-acp-planned-update",
            "handoff_id": identity.handoff_id,
            "start_nonce": identity.start_nonce,
            "phase": "finalized",
            "agent_pubkey": runtime_key.pubkey,
            "relay_sha256": relay_sha256,
            "written_at_unix_secs": 1,
            "membership_replay_from": 1,
            "channels": [],
        });
        fs::write(
            &paths.checkpoint,
            serde_json::to_vec(&document).expect("json"),
        )
        .expect("checkpoint");
        fs::set_permissions(&paths.checkpoint, fs::Permissions::from_mode(0o600))
            .expect("permissions");
        assert!(verify_handoff_checkpoint(&paths.checkpoint, &runtime_key, &identity).is_ok());

        let other =
            ManagedAgentRuntimeKey::new("bb".repeat(32), &runtime_key.relay_url).expect("other");
        assert!(verify_handoff_checkpoint(&paths.checkpoint, &other, &identity).is_err());
        let wrong_generation = PlannedUpdateIdentity {
            handoff_id: identity.handoff_id,
            start_nonce: "34".repeat(16),
        };
        assert!(
            verify_handoff_checkpoint(&paths.checkpoint, &runtime_key, &wrong_generation).is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn only_v3_finalized_checkpoint_can_complete_desktop_handoff() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = TempDir::new();
        let runtime_key = key();
        let paths = prepare_handoff_at(&temp.0, &runtime_key).expect("paths");
        let identity = identity();
        let relay_sha256 = hex::encode(Sha256::digest(runtime_key.relay_url.as_bytes()));
        let document = |version, phase: Option<&str>| {
            let mut value = serde_json::json!({
                "version": version,
                "kind": "buzz-acp-planned-update",
                "handoff_id": identity.handoff_id.clone(),
                "start_nonce": identity.start_nonce.clone(),
                "agent_pubkey": runtime_key.pubkey.clone(),
                "relay_sha256": relay_sha256.clone(),
                "written_at_unix_secs": 10,
                "membership_replay_from": 1,
                "channels": [],
            });
            if let Some(phase) = phase {
                value["phase"] = phase.into();
            }
            value
        };
        let write = |value: serde_json::Value| {
            fs::write(
                &paths.checkpoint,
                serde_json::to_vec(&value).expect("json"),
            )
            .expect("checkpoint");
            fs::set_permissions(&paths.checkpoint, fs::Permissions::from_mode(0o600))
                .expect("permissions");
        };

        write(document(3, Some("pre_quiesce")));
        assert!(
            verify_handoff_checkpoint(&paths.checkpoint, &runtime_key, &identity).is_err(),
            "crash-window pre-quiesce checkpoint must not complete the update"
        );
        write(document(2, None));
        assert!(
            verify_handoff_checkpoint(&paths.checkpoint, &runtime_key, &identity).is_err(),
            "legacy v2 recovery checkpoint must not complete a Desktop transaction"
        );
        write(document(3, Some("finalized")));
        assert!(verify_handoff_checkpoint(&paths.checkpoint, &runtime_key, &identity).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_rejects_malformed_or_duplicate_replay_state() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = TempDir::new();
        let runtime_key = key();
        let paths = prepare_handoff_at(&temp.0, &runtime_key).expect("paths");
        let identity = identity();
        let relay_sha256 = hex::encode(Sha256::digest(runtime_key.relay_url.as_bytes()));
        let channel_id = uuid::Uuid::new_v4();
        let document = serde_json::json!({
            "version": 3,
            "kind": "buzz-acp-planned-update",
            "handoff_id": identity.handoff_id,
            "start_nonce": identity.start_nonce,
            "phase": "finalized",
            "agent_pubkey": runtime_key.pubkey,
            "relay_sha256": relay_sha256,
            "written_at_unix_secs": 10,
            "membership_replay_from": 1,
            "channels": [
                {
                    "channel_id": channel_id,
                    "replay_from": 1,
                    "completed_event_ids": ["ab".repeat(32)],
                },
                {
                    "channel_id": channel_id,
                    "replay_from": 1,
                    "completed_event_ids": ["cd".repeat(32)],
                }
            ],
        });
        fs::write(
            &paths.checkpoint,
            serde_json::to_vec(&document).expect("json"),
        )
        .expect("checkpoint");
        fs::set_permissions(&paths.checkpoint, fs::Permissions::from_mode(0o600))
            .expect("permissions");

        assert!(verify_handoff_checkpoint(&paths.checkpoint, &runtime_key, &identity).is_err());
    }
}
