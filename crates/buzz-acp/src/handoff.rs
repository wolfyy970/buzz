//! Durable planned-update handoff for the ACP harness.
//!
//! The checkpoint deliberately stores replay metadata, not ACP sessions,
//! prompts, event bodies, credentials, or tool configuration. Recovery is
//! therefore at-least-once: only event IDs whose turns completed successfully
//! are skipped after replay; queued, failed, cancelled, panicked, or aborted
//! turns remain eligible for delivery.
//!
//! Wire compatibility: v1 request/checkpoint documents have no transaction
//! identity. A v2 request adds `handoff_id` (canonical random UUID) and
//! `expected_start_nonce` (32 lowercase hex characters); both checkpoint
//! writes echo them as `handoff_id` and `start_nonce`. Readers accept v1 and v2
//! but reject partial or cross-version identity fields. Rollouts must upgrade
//! the ACP reader before Desktop begins publishing v2 requests.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::pool::{PromptOutcome, PromptResult, PromptSource};
use crate::queue::FlushBatch;

const LEGACY_CHECKPOINT_VERSION: u32 = 1;
const CHECKPOINT_VERSION: u32 = 2;
const CHECKPOINT_KIND: &str = "buzz-acp-planned-update";
const LEGACY_REQUEST_VERSION: u32 = 1;
const REQUEST_VERSION: u32 = 2;
const REQUEST_KIND: &str = "buzz-acp-planned-update-request";
const CHECKPOINT_MAX_BYTES: u64 = 256 * 1024;
const REQUEST_MAX_BYTES: u64 = 1024;
const MAX_CHANNELS: usize = 256;
const MAX_PENDING_IDS_PER_CHANNEL: usize = 1024;
const MAX_COMPLETED_IDS_PER_CHANNEL: usize = 512;
const MAX_COMPLETED_IDS_TOTAL: usize = 2048;
const MAX_FUTURE_SKEW_SECS: u64 = 300;
pub(crate) const REPLAY_SKEW_SECS: u64 = 5;
/// Buzz accepts events within ±900 seconds of relay time, while its deferred
/// commit-time floor allows 960 seconds for validation and lock delay. Starting
/// every planned-handoff replay at least this far before cutover therefore
/// covers an event that commits after old intake stops with the oldest
/// `created_at` Buzz can persist. Keep this aligned with
/// `buzz_db::replica_fence::CREATED_AT_FLOOR_SECS`.
const HANDOFF_REPLAY_HORIZON_SECS: u64 = 960;

#[derive(Debug, Error)]
pub(crate) enum HandoffError {
    #[error("{0}")]
    Invalid(String),
    #[error("{operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid handoff JSON in {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

fn io_error(operation: &'static str, path: &Path, source: std::io::Error) -> HandoffError {
    HandoffError::Io {
        operation,
        path: path.display().to_string(),
        source,
    }
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_canonical_handoff_id(value: &str) -> Result<Uuid, HandoffError> {
    let parsed = Uuid::parse_str(value)
        .map_err(|_| HandoffError::Invalid("handoff ID must be a canonical UUID".into()))?;
    if parsed.hyphenated().to_string() != value
        || parsed.get_version() != Some(uuid::Version::Random)
    {
        return Err(HandoffError::Invalid(
            "handoff ID must be a canonical lowercase random UUID".into(),
        ));
    }
    Ok(parsed)
}

fn validate_identity_field_presence(
    value: &serde_json::Value,
    version: u32,
    legacy_version: u32,
    current_version: u32,
    fields: &[&str],
    label: &str,
) -> Result<(), HandoffError> {
    let object = value
        .as_object()
        .ok_or_else(|| HandoffError::Invalid(format!("{label} must be a JSON object")))?;
    match version {
        version
            if version == legacy_version
                && fields.iter().any(|field| object.contains_key(*field)) =>
        {
            Err(HandoffError::Invalid(format!(
                "legacy {label} must not contain v2 identity fields"
            )))
        }
        version
            if version == current_version
                && fields.iter().any(|field| !object.contains_key(*field)) =>
        {
            Err(HandoffError::Invalid(format!(
                "v2 {label} must contain every identity field"
            )))
        }
        _ => Ok(()),
    }
}

fn relay_fingerprint(relay_url: &str) -> String {
    hex::encode(Sha256::digest(relay_url.as_bytes()))
}

/// Identity binding for one planned-update transaction.
///
/// Version 1 predates process-generation binding. Version 2 checkpoints echo
/// the UUID and Desktop process generation from the accepted request. The
/// managed signal path waits for Desktop's v2 control file instead of
/// inventing another identity. Unmanaged/older launchers retain the v1 bare
/// signal behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckpointIdentity {
    LegacyV1,
    V2 {
        handoff_id: Uuid,
        start_nonce: String,
    },
}

impl CheckpointIdentity {
    pub(crate) fn for_bare_signal(start_nonce: &str) -> Result<Option<Self>, HandoffError> {
        if start_nonce.is_empty() {
            return Ok(Some(Self::LegacyV1));
        }
        if is_lower_hex(start_nonce, 32) {
            return Ok(None);
        }
        Err(HandoffError::Invalid(
            "managed planned-update signal has an invalid process start nonce".into(),
        ))
    }

    fn version(&self) -> u32 {
        match self {
            Self::LegacyV1 => LEGACY_CHECKPOINT_VERSION,
            Self::V2 { .. } => CHECKPOINT_VERSION,
        }
    }

    fn handoff_id(&self) -> Option<String> {
        match self {
            Self::LegacyV1 => None,
            Self::V2 { handoff_id, .. } => Some(handoff_id.hyphenated().to_string()),
        }
    }

    fn start_nonce(&self) -> Option<String> {
        match self {
            Self::LegacyV1 => None,
            Self::V2 { start_nonce, .. } => Some(start_nonce.clone()),
        }
    }
}

fn validate_absolute_leaf(path: &Path, label: &str) -> Result<(), HandoffError> {
    if !path.is_absolute() {
        return Err(HandoffError::Invalid(format!(
            "{label} path must be absolute: {}",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        HandoffError::Invalid(format!("{label} path has no parent: {}", path.display()))
    })?;
    if path.file_name().is_none() {
        return Err(HandoffError::Invalid(format!(
            "{label} path has no file name: {}",
            path.display()
        )));
    }
    let parent_metadata = std::fs::symlink_metadata(parent)
        .map_err(|error| io_error("inspect parent for", path, error))?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(HandoffError::Invalid(format!(
            "{label} parent must be a real directory, not a symlink: {}",
            parent.display()
        )));
    }
    validate_parent_security(parent, &parent_metadata)?;
    Ok(())
}

pub(crate) fn validate_configured_path(path: &Path) -> Result<(), HandoffError> {
    validate_absolute_leaf(path, "handoff")
}

#[cfg(unix)]
fn validate_parent_security(path: &Path, metadata: &Metadata) -> Result<(), HandoffError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let expected_uid = nix::unistd::geteuid().as_raw();
    if metadata.uid() != expected_uid {
        return Err(HandoffError::Invalid(format!(
            "handoff parent {} is not owned by the current user",
            path.display()
        )));
    }
    if metadata.permissions().mode() & 0o022 != 0 {
        return Err(HandoffError::Invalid(format!(
            "handoff parent {} must not be group/world writable",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_parent_security(_path: &Path, _metadata: &Metadata) -> Result<(), HandoffError> {
    Ok(())
}

#[cfg(unix)]
fn validate_file_security(path: &Path, metadata: &Metadata) -> Result<(), HandoffError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let expected_uid = nix::unistd::geteuid().as_raw();
    if metadata.uid() != expected_uid {
        return Err(HandoffError::Invalid(format!(
            "handoff file {} is not owned by the current user",
            path.display()
        )));
    }
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(HandoffError::Invalid(format!(
            "handoff file {} must have mode 0600",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_file_security(_path: &Path, _metadata: &Metadata) -> Result<(), HandoffError> {
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    length: u64,
    modified: Option<std::time::SystemTime>,
    content_sha256: [u8; 32],
}

fn file_identity(metadata: &Metadata, bytes: &[u8]) -> FileIdentity {
    let content_sha256 = Sha256::digest(bytes).into();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified: metadata.modified().ok(),
            content_sha256,
        }
    }
    #[cfg(not(unix))]
    {
        FileIdentity {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            content_sha256,
        }
    }
}

struct SecureBytes {
    bytes: Vec<u8>,
    identity: FileIdentity,
}

fn read_secure_file(path: &Path, max_bytes: u64) -> Result<Option<SecureBytes>, HandoffError> {
    validate_absolute_leaf(path, "handoff")?;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(HandoffError::Invalid(format!(
                "handoff path must not be a symlink: {}",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("inspect", path, error)),
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| io_error("open", path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error("inspect opened", path, error))?;
    if !metadata.is_file() {
        return Err(HandoffError::Invalid(format!(
            "handoff path must be a regular file: {}",
            path.display()
        )));
    }
    validate_file_security(path, &metadata)?;
    if metadata.len() > max_bytes {
        return Err(HandoffError::Invalid(format!(
            "handoff file {} exceeds {} byte limit",
            path.display(),
            max_bytes
        )));
    }

    let mut bytes = Vec::with_capacity((metadata.len().min(max_bytes)) as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read", path, error))?;
    if bytes.len() as u64 > max_bytes {
        return Err(HandoffError::Invalid(format!(
            "handoff file {} exceeds {} byte limit",
            path.display(),
            max_bytes
        )));
    }
    let identity = file_identity(&metadata, &bytes);
    Ok(Some(SecureBytes { bytes, identity }))
}

fn remove_if_unchanged(path: &Path, identity: &FileIdentity) -> Result<(), HandoffError> {
    let current = read_secure_file(path, identity.length.max(1))?.ok_or_else(|| {
        HandoffError::Invalid(format!(
            "handoff file disappeared before it could be consumed: {}",
            path.display()
        ))
    })?;
    if current.identity != *identity {
        return Err(HandoffError::Invalid(format!(
            "handoff file changed before it could be consumed: {}",
            path.display()
        )));
    }
    std::fs::remove_file(path).map_err(|error| io_error("remove", path, error))?;
    sync_parent(path)
}

fn sync_parent(path: &Path) -> Result<(), HandoffError> {
    let parent = path
        .parent()
        .ok_or_else(|| HandoffError::Invalid("handoff path has no parent".into()))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync parent for", path, error))
}

fn validate_existing_target(path: &Path) -> Result<(), HandoffError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(HandoffError::Invalid(format!(
                    "handoff target must be a regular file, not a symlink: {}",
                    path.display()
                )));
            }
            validate_file_security(path, &metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("inspect target", path, error)),
    }
}

fn atomic_write_secure(path: &Path, bytes: &[u8], max_bytes: u64) -> Result<(), HandoffError> {
    validate_absolute_leaf(path, "handoff")?;
    validate_existing_target(path)?;
    if bytes.len() as u64 > max_bytes {
        return Err(HandoffError::Invalid(format!(
            "handoff document exceeds {} byte limit",
            max_bytes
        )));
    }

    let parent = path
        .parent()
        .ok_or_else(|| HandoffError::Invalid("handoff path has no parent".into()))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| HandoffError::Invalid("handoff path file name is not UTF-8".into()))?;
    let temp_path = parent.join(format!(".{file_name}.tmp-{}", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW);
    }
    let mut temp = options
        .open(&temp_path)
        .map_err(|error| io_error("create temporary handoff file", &temp_path, error))?;
    let temp_path_guard = tempfile::TempPath::try_from_path(temp_path.clone())
        .map_err(|error| io_error("guard temporary handoff file", &temp_path, error))?;
    temp.write_all(bytes)
        .map_err(|error| io_error("write", &temp_path, error))?;
    temp.sync_all()
        .map_err(|error| io_error("sync", &temp_path, error))?;
    drop(temp);
    validate_existing_target(path)?;
    temp_path_guard
        .persist(path)
        .map_err(|error| io_error("replace", path, error.error))?;
    sync_parent(path)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointDocument {
    version: u32,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    handoff_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_nonce: Option<String>,
    agent_pubkey: String,
    relay_sha256: String,
    written_at_unix_secs: u64,
    membership_replay_from: u64,
    channels: Vec<ChannelCheckpoint>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelCheckpoint {
    channel_id: Uuid,
    replay_from: u64,
    completed_event_ids: Vec<String>,
}

impl CheckpointDocument {
    fn checkpoint_identity(&self) -> Result<CheckpointIdentity, HandoffError> {
        match (
            self.version,
            self.handoff_id.as_deref(),
            self.start_nonce.as_deref(),
        ) {
            (LEGACY_CHECKPOINT_VERSION, None, None) => Ok(CheckpointIdentity::LegacyV1),
            (CHECKPOINT_VERSION, Some(handoff_id), Some(start_nonce)) => {
                if !is_lower_hex(start_nonce, 32) {
                    return Err(HandoffError::Invalid(
                        "handoff checkpoint contains an invalid process start nonce".into(),
                    ));
                }
                Ok(CheckpointIdentity::V2 {
                    handoff_id: parse_canonical_handoff_id(handoff_id)?,
                    start_nonce: start_nonce.into(),
                })
            }
            (LEGACY_CHECKPOINT_VERSION, _, _) => Err(HandoffError::Invalid(
                "legacy handoff checkpoint must not contain v2 identity fields".into(),
            )),
            (CHECKPOINT_VERSION, _, _) => Err(HandoffError::Invalid(
                "v2 handoff checkpoint must contain handoff_id and start_nonce".into(),
            )),
            _ => Err(HandoffError::Invalid(
                "unsupported handoff checkpoint version".into(),
            )),
        }
    }

    fn validate(
        &self,
        expected_pubkey: &str,
        expected_relay_url: &str,
    ) -> Result<(), HandoffError> {
        if self.kind != CHECKPOINT_KIND {
            return Err(HandoffError::Invalid(
                "unsupported handoff checkpoint version or kind".into(),
            ));
        }
        self.checkpoint_identity()?;
        if !is_lower_hex(&self.agent_pubkey, 64)
            || self.agent_pubkey != expected_pubkey.to_ascii_lowercase()
        {
            return Err(HandoffError::Invalid(
                "handoff checkpoint belongs to a different agent".into(),
            ));
        }
        let expected_relay = relay_fingerprint(expected_relay_url);
        if !is_lower_hex(&self.relay_sha256, 64) || self.relay_sha256 != expected_relay {
            return Err(HandoffError::Invalid(
                "handoff checkpoint belongs to a different relay".into(),
            ));
        }
        let now = unix_now_secs();
        if self.written_at_unix_secs > now.saturating_add(MAX_FUTURE_SKEW_SECS)
            || self.membership_replay_from > self.written_at_unix_secs
        {
            return Err(HandoffError::Invalid(
                "handoff checkpoint contains an invalid replay timestamp".into(),
            ));
        }
        if self.channels.len() > MAX_CHANNELS {
            return Err(HandoffError::Invalid(format!(
                "handoff checkpoint exceeds {MAX_CHANNELS} channel limit"
            )));
        }
        let mut channels = HashSet::with_capacity(self.channels.len());
        let mut completed = HashSet::new();
        for channel in &self.channels {
            if !channels.insert(channel.channel_id) {
                return Err(HandoffError::Invalid(
                    "handoff checkpoint contains duplicate channels".into(),
                ));
            }
            if channel.replay_from > self.written_at_unix_secs {
                return Err(HandoffError::Invalid(
                    "handoff checkpoint channel replay timestamp is in the future".into(),
                ));
            }
            if channel.completed_event_ids.len() > MAX_COMPLETED_IDS_PER_CHANNEL {
                return Err(HandoffError::Invalid(format!(
                    "handoff checkpoint exceeds {MAX_COMPLETED_IDS_PER_CHANNEL} completed IDs per channel"
                )));
            }
            for id in &channel.completed_event_ids {
                if !is_lower_hex(id, 64) || !completed.insert(id) {
                    return Err(HandoffError::Invalid(
                        "handoff checkpoint contains an invalid or duplicate event ID".into(),
                    ));
                }
            }
        }
        if completed.len() > MAX_COMPLETED_IDS_TOTAL {
            return Err(HandoffError::Invalid(format!(
                "handoff checkpoint exceeds {MAX_COMPLETED_IDS_TOTAL} completed ID limit"
            )));
        }
        Ok(())
    }
}

pub(crate) struct LoadedCheckpoint {
    path: PathBuf,
    identity: FileIdentity,
    document: CheckpointDocument,
}

impl LoadedCheckpoint {
    pub(crate) fn recovery(&self) -> RecoveryState {
        RecoveryState {
            membership_replay_from: self.document.membership_replay_from,
            channel_floors: self
                .document
                .channels
                .iter()
                .map(|channel| (channel.channel_id, channel.replay_from))
                .collect(),
            completed_event_ids: self
                .document
                .channels
                .iter()
                .flat_map(|channel| channel.completed_event_ids.iter().cloned())
                .collect(),
        }
    }

    pub(crate) fn consume(self) -> Result<(), HandoffError> {
        remove_if_unchanged(&self.path, &self.identity)
    }

    #[cfg(test)]
    fn checkpoint_identity(&self) -> Result<CheckpointIdentity, HandoffError> {
        self.document.checkpoint_identity()
    }
}

#[derive(Default)]
pub(crate) struct RecoveryState {
    membership_replay_from: u64,
    channel_floors: HashMap<Uuid, u64>,
    completed_event_ids: HashSet<String>,
}

impl RecoveryState {
    pub(crate) fn membership_floor(&self) -> Option<u64> {
        (self.membership_replay_from != 0).then_some(self.membership_replay_from)
    }

    pub(crate) fn channel_floor(&self, channel_id: &Uuid) -> Option<u64> {
        self.channel_floors.get(channel_id).copied()
    }

    pub(crate) fn channels(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.channel_floors.keys().copied()
    }

    pub(crate) fn should_skip_completed(&self, event_id: &str) -> bool {
        self.completed_event_ids.contains(event_id)
    }
}

pub(crate) fn load_checkpoint(
    path: Option<&Path>,
    agent_pubkey: &str,
    relay_url: &str,
) -> Result<Option<LoadedCheckpoint>, HandoffError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let Some(secure) = read_secure_file(path, CHECKPOINT_MAX_BYTES)? else {
        return Ok(None);
    };
    let document: CheckpointDocument =
        serde_json::from_slice(&secure.bytes).map_err(|source| HandoffError::Json {
            path: path.display().to_string(),
            source,
        })?;
    let raw: serde_json::Value =
        serde_json::from_slice(&secure.bytes).map_err(|source| HandoffError::Json {
            path: path.display().to_string(),
            source,
        })?;
    validate_identity_field_presence(
        &raw,
        document.version,
        LEGACY_CHECKPOINT_VERSION,
        CHECKPOINT_VERSION,
        &["handoff_id", "start_nonce"],
        "handoff checkpoint",
    )?;
    document.validate(agent_pubkey, relay_url)?;
    Ok(Some(LoadedCheckpoint {
        path: path.to_path_buf(),
        identity: secure.identity,
        document,
    }))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    version: u32,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    handoff_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_start_nonce: Option<String>,
}

pub(crate) struct LoadedUpdateRequest {
    path: PathBuf,
    identity: FileIdentity,
    checkpoint_identity: CheckpointIdentity,
}

impl LoadedUpdateRequest {
    pub(crate) fn checkpoint_identity(&self) -> &CheckpointIdentity {
        &self.checkpoint_identity
    }

    pub(crate) fn consume(self) -> Result<(), HandoffError> {
        remove_if_unchanged(&self.path, &self.identity)
    }
}

pub(crate) fn load_update_request(
    path: &Path,
    runtime_start_nonce: &str,
) -> Result<Option<LoadedUpdateRequest>, HandoffError> {
    let Some(secure) = read_secure_file(path, REQUEST_MAX_BYTES)? else {
        return Ok(None);
    };
    if !runtime_start_nonce.is_empty() && !is_lower_hex(runtime_start_nonce, 32) {
        return Err(HandoffError::Invalid(
            "planned-update request cannot bind an invalid process start nonce".into(),
        ));
    }
    let request: UpdateRequest =
        serde_json::from_slice(&secure.bytes).map_err(|source| HandoffError::Json {
            path: path.display().to_string(),
            source,
        })?;
    let raw: serde_json::Value =
        serde_json::from_slice(&secure.bytes).map_err(|source| HandoffError::Json {
            path: path.display().to_string(),
            source,
        })?;
    validate_identity_field_presence(
        &raw,
        request.version,
        LEGACY_REQUEST_VERSION,
        REQUEST_VERSION,
        &["handoff_id", "expected_start_nonce"],
        "planned-update request",
    )?;
    if request.kind != REQUEST_KIND {
        return Err(HandoffError::Invalid(format!(
            "unsupported planned-update request in {}",
            path.display()
        )));
    }
    let checkpoint_identity = match (
        request.version,
        request.handoff_id.as_deref(),
        request.expected_start_nonce.as_deref(),
    ) {
        (LEGACY_REQUEST_VERSION, None, None) => CheckpointIdentity::LegacyV1,
        (REQUEST_VERSION, Some(handoff_id), Some(expected_start_nonce)) => {
            let handoff_id = parse_canonical_handoff_id(handoff_id)?;
            if !is_lower_hex(expected_start_nonce, 32) {
                return Err(HandoffError::Invalid(
                    "planned-update request contains an invalid expected start nonce".into(),
                ));
            }
            if runtime_start_nonce != expected_start_nonce {
                return Err(HandoffError::Invalid(
                    "planned-update request belongs to another process generation".into(),
                ));
            }
            CheckpointIdentity::V2 {
                handoff_id,
                start_nonce: expected_start_nonce.into(),
            }
        }
        (LEGACY_REQUEST_VERSION, _, _) => {
            return Err(HandoffError::Invalid(
                "legacy planned-update request must not contain v2 identity fields".into(),
            ));
        }
        (REQUEST_VERSION, _, _) => {
            return Err(HandoffError::Invalid(
                "v2 planned-update request must contain handoff_id and expected_start_nonce".into(),
            ));
        }
        _ => {
            return Err(HandoffError::Invalid(format!(
                "unsupported planned-update request in {}",
                path.display()
            )));
        }
    };
    Ok(Some(LoadedUpdateRequest {
        path: path.to_path_buf(),
        identity: secure.identity,
        checkpoint_identity,
    }))
}

#[derive(Clone)]
struct CompletedEvent {
    id: String,
    created_at: u64,
    sequence: u64,
}

#[derive(Default)]
struct ChannelState {
    pending: HashMap<String, u64>,
    conservative_pending_floor: Option<u64>,
    in_flight: HashSet<String>,
    completed: VecDeque<CompletedEvent>,
    completed_ids: HashSet<String>,
}

impl ChannelState {
    fn pending_floor(&self) -> Option<u64> {
        self.pending
            .values()
            .copied()
            .min()
            .into_iter()
            .chain(self.conservative_pending_floor)
            .min()
    }
}

#[derive(Default)]
pub(crate) struct HandoffTracker {
    channels: HashMap<Uuid, ChannelState>,
    channel_overflow: bool,
    completed_total: usize,
    next_sequence: u64,
}

pub(crate) struct CheckpointWriteParams<'a> {
    pub(crate) path: &'a Path,
    pub(crate) identity: &'a CheckpointIdentity,
    pub(crate) agent_pubkey: &'a str,
    pub(crate) relay_url: &'a str,
    pub(crate) subscribed_channels: &'a HashSet<Uuid>,
    pub(crate) cutover_time: u64,
    pub(crate) membership_relay_floor: Option<u64>,
    pub(crate) relay_channel_floors: &'a HashMap<Uuid, u64>,
    pub(crate) relay_fallback_floor: Option<u64>,
}

impl HandoffTracker {
    pub(crate) fn has_pending_work(&self) -> bool {
        self.channel_overflow
            || self.channels.values().any(|state| {
                !state.pending.is_empty() || state.conservative_pending_floor.is_some()
            })
    }

    pub(crate) fn record_pending(&mut self, channel_id: Uuid, event_id: String, created_at: u64) {
        if !self.channels.contains_key(&channel_id) && self.channels.len() >= MAX_CHANNELS {
            self.channel_overflow = true;
            return;
        }
        let state = self.channels.entry(channel_id).or_default();
        if state.pending.contains_key(&event_id) || state.completed_ids.contains(&event_id) {
            return;
        }
        if state.pending.len() < MAX_PENDING_IDS_PER_CHANNEL {
            state.pending.insert(event_id, created_at);
        } else {
            state.conservative_pending_floor = Some(
                state
                    .conservative_pending_floor
                    .map_or(created_at, |floor| floor.min(created_at)),
            );
        }
    }

    pub(crate) fn record_dispatched(&mut self, batch: &FlushBatch) {
        let Some(state) = self.channels.get_mut(&batch.channel_id) else {
            return;
        };
        state.in_flight.clear();
        state.in_flight.extend(
            batch
                .events
                .iter()
                .chain(batch.cancelled_events.iter())
                .map(|event| event.event.id.to_hex()),
        );
    }

    pub(crate) fn record_steered(&mut self, channel_id: Uuid, event_id: &str) {
        if let Some(state) = self.channels.get_mut(&channel_id) {
            if state.pending.contains_key(event_id) {
                state.in_flight.insert(event_id.to_string());
            }
        }
    }

    pub(crate) fn record_result(&mut self, result: &PromptResult) {
        let PromptSource::Channel(channel_id) = &result.source else {
            return;
        };
        self.record_channel_outcome(*channel_id, matches!(result.outcome, PromptOutcome::Ok(_)));
    }

    fn record_channel_outcome(&mut self, channel_id: Uuid, completed_successfully: bool) {
        let Some(state) = self.channels.get_mut(&channel_id) else {
            return;
        };
        let event_ids = std::mem::take(&mut state.in_flight);
        if !completed_successfully {
            return;
        }
        let mut completed = Vec::new();
        for id in event_ids {
            if let Some(created_at) = state.pending.remove(&id) {
                completed.push((id, created_at));
            }
        }
        for (id, created_at) in completed {
            self.push_completed(channel_id, id, created_at);
        }
    }

    fn push_completed(&mut self, channel_id: Uuid, id: String, created_at: u64) {
        let state = self.channels.entry(channel_id).or_default();
        if !state.completed_ids.insert(id.clone()) {
            return;
        }
        self.next_sequence = self.next_sequence.wrapping_add(1);
        state.completed.push_back(CompletedEvent {
            id,
            created_at,
            sequence: self.next_sequence,
        });
        self.completed_total += 1;
        while state.completed.len() > MAX_COMPLETED_IDS_PER_CHANNEL {
            if let Some(removed) = state.completed.pop_front() {
                state.completed_ids.remove(&removed.id);
                self.completed_total = self.completed_total.saturating_sub(1);
            }
        }
        self.trim_completed_total();
    }

    fn trim_completed_total(&mut self) {
        while self.completed_total > MAX_COMPLETED_IDS_TOTAL {
            let oldest_channel = self
                .channels
                .iter()
                .filter_map(|(channel, state)| {
                    state
                        .completed
                        .front()
                        .map(|event| (*channel, event.sequence))
                })
                .min_by_key(|(_, sequence)| *sequence)
                .map(|(channel, _)| channel);
            let Some(channel_id) = oldest_channel else {
                self.completed_total = 0;
                break;
            };
            if let Some(state) = self.channels.get_mut(&channel_id) {
                if let Some(removed) = state.completed.pop_front() {
                    state.completed_ids.remove(&removed.id);
                    self.completed_total = self.completed_total.saturating_sub(1);
                }
            }
        }
    }

    pub(crate) fn write_checkpoint(
        &self,
        params: CheckpointWriteParams<'_>,
    ) -> Result<(), HandoffError> {
        if self.channel_overflow || params.subscribed_channels.len() > MAX_CHANNELS {
            return Err(HandoffError::Invalid(format!(
                "cannot create a bounded-replay handoff for more than {MAX_CHANNELS} channels"
            )));
        }
        let mut channel_ids: Vec<Uuid> = params.subscribed_channels.iter().copied().collect();
        channel_ids.sort_unstable();
        let mut channels = Vec::with_capacity(channel_ids.len());
        let cutover_floor = params
            .cutover_time
            .saturating_sub(HANDOFF_REPLAY_HORIZON_SECS);
        for channel_id in channel_ids {
            let state = self.channels.get(&channel_id);
            let replay_from = state
                .and_then(ChannelState::pending_floor)
                .into_iter()
                .chain(params.relay_channel_floors.get(&channel_id).copied())
                .chain(params.relay_fallback_floor)
                .chain(std::iter::once(cutover_floor))
                .min()
                .unwrap_or(cutover_floor);
            let overlap_floor = replay_from.saturating_sub(REPLAY_SKEW_SECS);
            let completed_event_ids = state
                .into_iter()
                .flat_map(|state| state.completed.iter())
                .filter(|event| event.created_at >= overlap_floor)
                .map(|event| event.id.clone())
                .collect();
            channels.push(ChannelCheckpoint {
                channel_id,
                replay_from,
                completed_event_ids,
            });
        }
        let document = CheckpointDocument {
            version: params.identity.version(),
            kind: CHECKPOINT_KIND.into(),
            handoff_id: params.identity.handoff_id(),
            start_nonce: params.identity.start_nonce(),
            agent_pubkey: params.agent_pubkey.to_ascii_lowercase(),
            relay_sha256: relay_fingerprint(params.relay_url),
            written_at_unix_secs: unix_now_secs(),
            membership_replay_from: params
                .membership_relay_floor
                .unwrap_or(cutover_floor)
                .min(cutover_floor),
            channels,
        };
        document.validate(params.agent_pubkey, params.relay_url)?;
        let bytes = serde_json::to_vec(&document).map_err(|source| HandoffError::Json {
            path: params.path.display().to_string(),
            source,
        })?;
        atomic_write_secure(params.path, &bytes, CHECKPOINT_MAX_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("buzz-acp-handoff-test-{}", Uuid::new_v4()));
            std::fs::create_dir(&path).expect("create temp directory");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                    .expect("secure temp directory");
            }
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn id(value: u64) -> String {
        format!("{value:064x}")
    }

    fn pubkey() -> String {
        "11".repeat(32)
    }

    fn start_nonce(value: u8) -> String {
        format!("{value:02x}").repeat(16)
    }

    fn v2_identity() -> CheckpointIdentity {
        CheckpointIdentity::V2 {
            handoff_id: Uuid::new_v4(),
            start_nonce: start_nonce(0x22),
        }
    }

    #[test]
    fn same_second_pending_event_is_not_hidden_by_completed_id() {
        let channel = Uuid::new_v4();
        let mut tracker = HandoffTracker::default();
        tracker.record_pending(channel, id(1), 100);
        tracker.record_pending(channel, id(2), 100);
        let state = tracker.channels.get_mut(&channel).unwrap();
        state.pending.remove(&id(1));
        tracker.push_completed(channel, id(1), 100);

        assert_eq!(
            tracker.channels[&channel].pending_floor(),
            Some(100),
            "same-second pending work must keep the replay floor inclusive"
        );
        assert!(!tracker.channels[&channel].completed_ids.contains(&id(2)));
    }

    #[test]
    fn multiple_channels_keep_independent_replay_floors() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let mut tracker = HandoffTracker::default();
        tracker.record_pending(first, id(1), 900);
        tracker.record_pending(second, id(2), 100);

        assert_eq!(tracker.channels[&first].pending_floor(), Some(900));
        assert_eq!(tracker.channels[&second].pending_floor(), Some(100));
    }

    #[test]
    fn completed_id_truncation_is_conservative() {
        let channel = Uuid::new_v4();
        let mut tracker = HandoffTracker::default();
        for value in 0..=MAX_COMPLETED_IDS_PER_CHANNEL {
            tracker.push_completed(channel, id(value as u64), 100);
        }
        let state = &tracker.channels[&channel];
        assert_eq!(state.completed.len(), MAX_COMPLETED_IDS_PER_CHANNEL);
        assert!(!state.completed_ids.contains(&id(0)));
        assert!(state
            .completed_ids
            .contains(&id(MAX_COMPLETED_IDS_PER_CHANNEL as u64)));
    }

    #[test]
    fn queued_and_aborted_work_remains_pending() {
        let channel = Uuid::new_v4();
        let mut tracker = HandoffTracker::default();
        tracker.record_pending(channel, id(7), 77);
        assert_eq!(tracker.channels[&channel].pending_floor(), Some(77));
        assert!(tracker.channels[&channel].completed.is_empty());
    }

    #[test]
    fn active_completion_is_skipped_but_aborted_turn_is_replayed() {
        let completed_channel = Uuid::new_v4();
        let aborted_channel = Uuid::new_v4();
        let mut tracker = HandoffTracker::default();
        tracker.record_pending(completed_channel, id(1), 50);
        tracker.record_pending(aborted_channel, id(2), 60);
        tracker
            .channels
            .get_mut(&completed_channel)
            .unwrap()
            .in_flight = HashSet::from([id(1)]);
        tracker
            .channels
            .get_mut(&aborted_channel)
            .unwrap()
            .in_flight = HashSet::from([id(2)]);

        tracker.record_channel_outcome(completed_channel, true);
        tracker.record_channel_outcome(aborted_channel, false);

        assert!(!tracker.channels[&completed_channel]
            .pending
            .contains_key(&id(1)));
        assert!(tracker.channels[&completed_channel]
            .completed_ids
            .contains(&id(1)));
        assert!(tracker.channels[&aborted_channel]
            .pending
            .contains_key(&id(2)));
        assert!(!tracker.channels[&aborted_channel]
            .completed_ids
            .contains(&id(2)));
    }

    #[test]
    fn legacy_v1_checkpoint_recovery_remains_compatible() {
        let temp = TempDir::new();
        let path = temp.0.join("checkpoint.json");
        let channel = Uuid::new_v4();
        let mut subscribed = HashSet::new();
        subscribed.insert(channel);
        let mut tracker = HandoffTracker::default();
        tracker.record_pending(channel, id(9), 1_090);
        tracker
            .write_checkpoint(CheckpointWriteParams {
                path: &path,
                identity: &CheckpointIdentity::LegacyV1,
                agent_pubkey: &pubkey(),
                relay_url: "wss://relay.example",
                subscribed_channels: &subscribed,
                cutover_time: 2_000,
                membership_relay_floor: Some(1_900),
                relay_channel_floors: &HashMap::new(),
                relay_fallback_floor: None,
            })
            .unwrap();

        let loaded = load_checkpoint(Some(&path), &pubkey(), "wss://relay.example")
            .unwrap()
            .unwrap();
        assert_eq!(
            loaded.checkpoint_identity().unwrap(),
            CheckpointIdentity::LegacyV1
        );
        let recovery = loaded.recovery();
        assert_eq!(recovery.channel_floor(&channel), Some(1_040));
        assert_eq!(recovery.membership_floor(), Some(1_040));
        let wire: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(wire["version"], LEGACY_CHECKPOINT_VERSION);
        assert!(wire.get("handoff_id").is_none());
        assert!(wire.get("start_nonce").is_none());
        loaded.consume().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn corrupt_and_oversized_checkpoints_are_rejected_without_deletion() {
        let temp = TempDir::new();
        let corrupt = temp.0.join("corrupt.json");
        atomic_write_secure(&corrupt, b"not json", CHECKPOINT_MAX_BYTES).unwrap();
        assert!(load_checkpoint(Some(&corrupt), &pubkey(), "wss://relay.example").is_err());
        assert!(corrupt.exists());

        let oversized = temp.0.join("oversized.json");
        atomic_write_secure(
            &oversized,
            &vec![b' '; CHECKPOINT_MAX_BYTES as usize],
            CHECKPOINT_MAX_BYTES,
        )
        .unwrap();
        let mut file = OpenOptions::new().append(true).open(&oversized).unwrap();
        file.write_all(b" ").unwrap();
        assert!(load_checkpoint(Some(&oversized), &pubkey(), "wss://relay.example").is_err());
        assert!(oversized.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_checkpoint_is_rejected_without_touching_target() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let target = temp.0.join("target");
        let link = temp.0.join("checkpoint.json");
        std::fs::write(&target, b"sentinel").unwrap();
        symlink(&target, &link).unwrap();

        assert!(load_checkpoint(Some(&link), &pubkey(), "wss://relay.example").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn group_readable_checkpoint_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new();
        let path = temp.0.join("checkpoint.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(load_checkpoint(Some(&path), &pubkey(), "wss://relay.example").is_err());
    }

    #[test]
    fn update_request_is_strict_bounded_and_one_shot() {
        let temp = TempDir::new();
        let path = temp.0.join("request.json");
        let bytes = serde_json::to_vec(&UpdateRequest {
            version: LEGACY_REQUEST_VERSION,
            kind: REQUEST_KIND.into(),
            handoff_id: None,
            expected_start_nonce: None,
        })
        .unwrap();
        atomic_write_secure(&path, &bytes, REQUEST_MAX_BYTES).unwrap();
        let runtime_start_nonce = start_nonce(0x12);
        let request = load_update_request(&path, &runtime_start_nonce)
            .unwrap()
            .unwrap();
        assert_eq!(request.checkpoint_identity(), &CheckpointIdentity::LegacyV1);
        assert!(
            path.exists(),
            "request remains durable until handoff commit"
        );
        request.consume().unwrap();
        assert!(!path.exists());
        assert!(load_update_request(&path, &runtime_start_nonce)
            .unwrap()
            .is_none());
    }

    #[test]
    fn legacy_v1_documents_reject_explicit_v2_identity_fields() {
        let temp = TempDir::new();
        let request_path = temp.0.join("request.json");
        atomic_write_secure(
            &request_path,
            br#"{"version":1,"kind":"buzz-acp-planned-update-request","handoff_id":null,"expected_start_nonce":null}"#,
            REQUEST_MAX_BYTES,
        )
        .unwrap();
        assert!(load_update_request(&request_path, "").is_err());

        let checkpoint_path = temp.0.join("checkpoint.json");
        let checkpoint = serde_json::json!({
            "version": 1,
            "kind": CHECKPOINT_KIND,
            "handoff_id": null,
            "start_nonce": null,
            "agent_pubkey": pubkey(),
            "relay_sha256": relay_fingerprint("wss://relay.example"),
            "written_at_unix_secs": 100,
            "membership_replay_from": 100,
            "channels": [],
        });
        atomic_write_secure(
            &checkpoint_path,
            &serde_json::to_vec(&checkpoint).unwrap(),
            CHECKPOINT_MAX_BYTES,
        )
        .unwrap();
        assert!(load_checkpoint(Some(&checkpoint_path), &pubkey(), "wss://relay.example").is_err());
    }

    #[test]
    fn malformed_managed_start_nonce_refuses_even_a_legacy_request() {
        let temp = TempDir::new();
        let path = temp.0.join("request.json");
        atomic_write_secure(
            &path,
            br#"{"version":1,"kind":"buzz-acp-planned-update-request"}"#,
            REQUEST_MAX_BYTES,
        )
        .unwrap();

        assert!(load_update_request(&path, "malformed-generation").is_err());
        assert!(path.exists());
    }

    #[test]
    fn update_request_in_place_rewrite_is_not_consumed() {
        let temp = TempDir::new();
        let path = temp.0.join("request.json");
        let bytes = serde_json::to_vec(&UpdateRequest {
            version: LEGACY_REQUEST_VERSION,
            kind: REQUEST_KIND.into(),
            handoff_id: None,
            expected_start_nonce: None,
        })
        .unwrap();
        atomic_write_secure(&path, &bytes, REQUEST_MAX_BYTES).unwrap();
        let request = load_update_request(&path, "").unwrap().unwrap();

        let mut changed = bytes;
        let last = changed.last_mut().expect("request JSON is non-empty");
        *last = if *last == b'}' { b' ' } else { b'}' };
        std::fs::write(&path, &changed).unwrap();

        assert!(request.consume().is_err());
        assert!(path.exists(), "changed request must remain for inspection");
    }

    #[test]
    fn v2_request_for_another_process_generation_is_refused() {
        let temp = TempDir::new();
        let path = temp.0.join("request.json");
        let expected_start_nonce = start_nonce(0x33);
        let bytes = serde_json::to_vec(&UpdateRequest {
            version: REQUEST_VERSION,
            kind: REQUEST_KIND.into(),
            handoff_id: Some(Uuid::new_v4().hyphenated().to_string()),
            expected_start_nonce: Some(expected_start_nonce.clone()),
        })
        .unwrap();
        atomic_write_secure(&path, &bytes, REQUEST_MAX_BYTES).unwrap();

        let error = load_update_request(&path, &start_nonce(0x44))
            .err()
            .expect("wrong generation must be refused");
        assert!(error.to_string().contains("another process generation"));
        assert!(path.exists(), "refused request must remain unconsumed");
        assert!(
            !temp.0.join("checkpoint.json").exists(),
            "refusing another generation must not create a checkpoint"
        );

        let request = load_update_request(&path, &expected_start_nonce)
            .unwrap()
            .unwrap();
        assert!(matches!(
            request.checkpoint_identity(),
            CheckpointIdentity::V2 { start_nonce, .. } if start_nonce == &expected_start_nonce
        ));
    }

    #[test]
    fn malformed_v2_requests_are_rejected_without_consumption() {
        let temp = TempDir::new();
        let path = temp.0.join("request.json");
        let expected_start_nonce = start_nonce(0x66);
        let handoff_id = Uuid::new_v4();
        let cases = [
            serde_json::json!({
                "version": REQUEST_VERSION,
                "kind": REQUEST_KIND,
                "handoff_id": handoff_id.hyphenated().to_string(),
            }),
            serde_json::json!({
                "version": REQUEST_VERSION,
                "kind": REQUEST_KIND,
                "handoff_id": null,
                "expected_start_nonce": expected_start_nonce.clone(),
            }),
            serde_json::json!({
                "version": REQUEST_VERSION,
                "kind": REQUEST_KIND,
                "handoff_id": handoff_id.simple().to_string(),
                "expected_start_nonce": expected_start_nonce.clone(),
            }),
            serde_json::json!({
                "version": REQUEST_VERSION,
                "kind": REQUEST_KIND,
                "handoff_id": Uuid::nil().hyphenated().to_string(),
                "expected_start_nonce": expected_start_nonce.clone(),
            }),
            serde_json::json!({
                "version": REQUEST_VERSION,
                "kind": REQUEST_KIND,
                "handoff_id": handoff_id.hyphenated().to_string(),
                "expected_start_nonce": "AB".repeat(16),
            }),
            serde_json::json!({
                "version": REQUEST_VERSION,
                "kind": REQUEST_KIND,
                "handoff_id": handoff_id.hyphenated().to_string(),
                "expected_start_nonce": expected_start_nonce.clone(),
                "unexpected": true,
            }),
        ];

        for document in cases {
            let bytes = serde_json::to_vec(&document).unwrap();
            atomic_write_secure(&path, &bytes, REQUEST_MAX_BYTES).unwrap();
            assert!(load_update_request(&path, &expected_start_nonce).is_err());
            assert!(path.exists(), "invalid request must remain unconsumed");
        }
    }

    #[test]
    fn changed_v2_request_is_not_consumed_as_the_loaded_transaction() {
        let temp = TempDir::new();
        let path = temp.0.join("request.json");
        let expected_start_nonce = start_nonce(0x55);
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let request_bytes = |handoff_id: Uuid| {
            serde_json::to_vec(&UpdateRequest {
                version: REQUEST_VERSION,
                kind: REQUEST_KIND.into(),
                handoff_id: Some(handoff_id.hyphenated().to_string()),
                expected_start_nonce: Some(expected_start_nonce.clone()),
            })
            .unwrap()
        };
        atomic_write_secure(&path, &request_bytes(first_id), REQUEST_MAX_BYTES).unwrap();
        let request = load_update_request(&path, &expected_start_nonce)
            .unwrap()
            .unwrap();

        atomic_write_secure(&path, &request_bytes(second_id), REQUEST_MAX_BYTES).unwrap();

        assert!(request.consume().is_err());
        let replacement = load_update_request(&path, &expected_start_nonce)
            .unwrap()
            .unwrap();
        assert_eq!(
            replacement.checkpoint_identity(),
            &CheckpointIdentity::V2 {
                handoff_id: second_id,
                start_nonce: expected_start_nonce,
            }
        );
    }

    #[test]
    fn v2_checkpoint_echoes_transaction_identity_across_rewrites() {
        let temp = TempDir::new();
        let path = temp.0.join("checkpoint.json");
        let channel = Uuid::new_v4();
        let subscribed = HashSet::from([channel]);
        let tracker = HandoffTracker::default();
        let identity = v2_identity();

        for cutover_time in [2_000, 2_100] {
            tracker
                .write_checkpoint(CheckpointWriteParams {
                    path: &path,
                    identity: &identity,
                    agent_pubkey: &pubkey(),
                    relay_url: "wss://relay.example",
                    subscribed_channels: &subscribed,
                    cutover_time,
                    membership_relay_floor: None,
                    relay_channel_floors: &HashMap::new(),
                    relay_fallback_floor: None,
                })
                .unwrap();
            let loaded = load_checkpoint(Some(&path), &pubkey(), "wss://relay.example")
                .unwrap()
                .unwrap();
            assert_eq!(loaded.checkpoint_identity().unwrap(), identity);
            let wire: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(wire["version"], CHECKPOINT_VERSION);
            let CheckpointIdentity::V2 {
                handoff_id,
                start_nonce,
            } = &identity
            else {
                unreachable!();
            };
            assert_eq!(wire["handoff_id"], handoff_id.hyphenated().to_string());
            assert_eq!(wire["start_nonce"].as_str(), Some(start_nonce.as_str()));
        }
    }

    #[test]
    fn partial_and_noncanonical_v2_checkpoint_identities_are_rejected() {
        let temp = TempDir::new();
        let path = temp.0.join("checkpoint.json");
        let identity = v2_identity();
        let CheckpointIdentity::V2 {
            handoff_id,
            start_nonce,
        } = identity
        else {
            unreachable!();
        };
        let document = CheckpointDocument {
            version: CHECKPOINT_VERSION,
            kind: CHECKPOINT_KIND.into(),
            handoff_id: Some(handoff_id.simple().to_string()),
            start_nonce: Some(start_nonce),
            agent_pubkey: pubkey(),
            relay_sha256: relay_fingerprint("wss://relay.example"),
            written_at_unix_secs: 100,
            membership_replay_from: 100,
            channels: vec![],
        };
        let bytes = serde_json::to_vec(&document).unwrap();
        atomic_write_secure(&path, &bytes, CHECKPOINT_MAX_BYTES).unwrap();
        assert!(load_checkpoint(Some(&path), &pubkey(), "wss://relay.example").is_err());
        assert!(path.exists());

        let partial = CheckpointDocument {
            version: CHECKPOINT_VERSION,
            kind: CHECKPOINT_KIND.into(),
            handoff_id: Some(handoff_id.hyphenated().to_string()),
            start_nonce: None,
            agent_pubkey: pubkey(),
            relay_sha256: relay_fingerprint("wss://relay.example"),
            written_at_unix_secs: 100,
            membership_replay_from: 100,
            channels: vec![],
        };
        let bytes = serde_json::to_vec(&partial).unwrap();
        atomic_write_secure(&path, &bytes, CHECKPOINT_MAX_BYTES).unwrap();
        assert!(load_checkpoint(Some(&path), &pubkey(), "wss://relay.example").is_err());
        assert!(path.exists());
    }

    #[test]
    fn bare_signal_is_legacy_only_without_a_managed_process_generation() {
        let nonce = start_nonce(0x77);
        assert_eq!(
            CheckpointIdentity::for_bare_signal("").unwrap(),
            Some(CheckpointIdentity::LegacyV1)
        );
        assert_eq!(CheckpointIdentity::for_bare_signal(&nonce).unwrap(), None);
        assert!(CheckpointIdentity::for_bare_signal("malformed-generation").is_err());
    }

    #[test]
    fn serialized_checkpoint_contains_no_relay_or_secret_material() {
        let document = CheckpointDocument {
            version: LEGACY_CHECKPOINT_VERSION,
            kind: CHECKPOINT_KIND.into(),
            handoff_id: None,
            start_nonce: None,
            agent_pubkey: pubkey(),
            relay_sha256: relay_fingerprint("wss://user:secret@relay.example?token=hidden"),
            written_at_unix_secs: 100,
            membership_replay_from: 100,
            channels: vec![],
        };
        let serialized = serde_json::to_string(&document).unwrap();
        for secret in ["user", "secret", "token", "hidden", "wss://"] {
            assert!(!serialized.contains(secret));
        }
    }
}
