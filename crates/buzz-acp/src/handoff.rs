//! Durable planned-update handoff for the ACP harness.
//!
//! The checkpoint deliberately stores replay metadata, not ACP sessions,
//! prompts, event bodies, credentials, or tool configuration. Recovery is
//! therefore at-least-once: only event IDs whose turns completed successfully
//! are skipped after replay; queued, failed, cancelled, panicked, or aborted
//! turns remain eligible for delivery.

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

const CHECKPOINT_VERSION: u32 = 1;
const CHECKPOINT_KIND: &str = "buzz-acp-planned-update";
const REQUEST_VERSION: u32 = 1;
const REQUEST_KIND: &str = "buzz-acp-planned-update-request";
const CHECKPOINT_MAX_BYTES: u64 = 256 * 1024;
const REQUEST_MAX_BYTES: u64 = 1024;
const MAX_CHANNELS: usize = 256;
const MAX_PENDING_IDS_PER_CHANNEL: usize = 1024;
const MAX_COMPLETED_IDS_PER_CHANNEL: usize = 512;
const MAX_COMPLETED_IDS_TOTAL: usize = 2048;
const MAX_FUTURE_SKEW_SECS: u64 = 300;
pub(crate) const REPLAY_SKEW_SECS: u64 = 5;

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

fn relay_fingerprint(relay_url: &str) -> String {
    hex::encode(Sha256::digest(relay_url.as_bytes()))
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
    #[cfg(not(unix))]
    length: u64,
    #[cfg(not(unix))]
    modified: Option<std::time::SystemTime>,
}

fn file_identity(metadata: &Metadata) -> FileIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
    #[cfg(not(unix))]
    {
        FileIdentity {
            length: metadata.len(),
            modified: metadata.modified().ok(),
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

    let identity = file_identity(&metadata);
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
    Ok(Some(SecureBytes { bytes, identity }))
}

fn remove_if_unchanged(path: &Path, identity: &FileIdentity) -> Result<(), HandoffError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| io_error("reinspect", path, error))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || file_identity(&metadata) != *identity
    {
        return Err(HandoffError::Invalid(format!(
            "handoff file changed before it could be consumed: {}",
            path.display()
        )));
    }
    validate_file_security(path, &metadata)?;
    std::fs::remove_file(path).map_err(|error| io_error("remove", path, error))
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
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync parent for", path, error))?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointDocument {
    version: u32,
    kind: String,
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
    fn validate(
        &self,
        expected_pubkey: &str,
        expected_relay_url: &str,
    ) -> Result<(), HandoffError> {
        if self.version != CHECKPOINT_VERSION || self.kind != CHECKPOINT_KIND {
            return Err(HandoffError::Invalid(
                "unsupported handoff checkpoint version or kind".into(),
            ));
        }
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
}

pub(crate) struct LoadedUpdateRequest {
    path: PathBuf,
    identity: FileIdentity,
}

impl LoadedUpdateRequest {
    pub(crate) fn consume(self) -> Result<(), HandoffError> {
        remove_if_unchanged(&self.path, &self.identity)
    }
}

pub(crate) fn load_update_request(
    path: &Path,
) -> Result<Option<LoadedUpdateRequest>, HandoffError> {
    let Some(secure) = read_secure_file(path, REQUEST_MAX_BYTES)? else {
        return Ok(None);
    };
    let request: UpdateRequest =
        serde_json::from_slice(&secure.bytes).map_err(|source| HandoffError::Json {
            path: path.display().to_string(),
            source,
        })?;
    if request.version != REQUEST_VERSION || request.kind != REQUEST_KIND {
        return Err(HandoffError::Invalid(format!(
            "unsupported planned-update request in {}",
            path.display()
        )));
    }
    Ok(Some(LoadedUpdateRequest {
        path: path.to_path_buf(),
        identity: secure.identity,
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
                "cannot create lossless handoff for more than {MAX_CHANNELS} channels"
            )));
        }
        let mut channel_ids: Vec<Uuid> = params.subscribed_channels.iter().copied().collect();
        channel_ids.sort_unstable();
        let mut channels = Vec::with_capacity(channel_ids.len());
        for channel_id in channel_ids {
            let state = self.channels.get(&channel_id);
            let replay_from = state
                .and_then(ChannelState::pending_floor)
                .into_iter()
                .chain(params.relay_channel_floors.get(&channel_id).copied())
                .chain(params.relay_fallback_floor)
                .chain(std::iter::once(params.cutover_time))
                .min()
                .unwrap_or(params.cutover_time);
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
            version: CHECKPOINT_VERSION,
            kind: CHECKPOINT_KIND.into(),
            agent_pubkey: params.agent_pubkey.to_ascii_lowercase(),
            relay_sha256: relay_fingerprint(params.relay_url),
            written_at_unix_secs: unix_now_secs(),
            membership_replay_from: params
                .membership_relay_floor
                .unwrap_or(params.cutover_time)
                .min(params.cutover_time),
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
    fn checkpoint_round_trip_binds_agent_and_relay() {
        let temp = TempDir::new();
        let path = temp.0.join("checkpoint.json");
        let channel = Uuid::new_v4();
        let mut subscribed = HashSet::new();
        subscribed.insert(channel);
        let mut tracker = HandoffTracker::default();
        tracker.record_pending(channel, id(9), 90);
        tracker
            .write_checkpoint(CheckpointWriteParams {
                path: &path,
                agent_pubkey: &pubkey(),
                relay_url: "wss://relay.example",
                subscribed_channels: &subscribed,
                cutover_time: 100,
                membership_relay_floor: Some(95),
                relay_channel_floors: &HashMap::new(),
                relay_fallback_floor: None,
            })
            .unwrap();

        let loaded = load_checkpoint(Some(&path), &pubkey(), "wss://relay.example")
            .unwrap()
            .unwrap();
        let recovery = loaded.recovery();
        assert_eq!(recovery.channel_floor(&channel), Some(90));
        assert_eq!(recovery.membership_floor(), Some(95));
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
            version: REQUEST_VERSION,
            kind: REQUEST_KIND.into(),
        })
        .unwrap();
        atomic_write_secure(&path, &bytes, REQUEST_MAX_BYTES).unwrap();
        let request = load_update_request(&path).unwrap().unwrap();
        assert!(
            path.exists(),
            "request remains durable until handoff commit"
        );
        request.consume().unwrap();
        assert!(!path.exists());
        assert!(load_update_request(&path).unwrap().is_none());
    }

    #[test]
    fn serialized_checkpoint_contains_no_relay_or_secret_material() {
        let document = CheckpointDocument {
            version: CHECKPOINT_VERSION,
            kind: CHECKPOINT_KIND.into(),
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
