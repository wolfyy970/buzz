use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{HashMap, HashSet},
    process::ExitStatus,
};

use super::ManagedAgentProcess;

/// Canonical identity of one managed-agent harness on one relay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentRuntimeKey {
    pub pubkey: String,
    pub relay_url: String,
}

impl ManagedAgentRuntimeKey {
    pub fn new(pubkey: impl Into<String>, relay_url: &str) -> Result<Self, String> {
        let pubkey = pubkey.into();
        if pubkey.len() != 64 || !pubkey.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("managed-agent pubkey must be 64 hexadecimal characters".into());
        }
        Ok(Self {
            pubkey: pubkey.to_ascii_lowercase(),
            relay_url: buzz_core_pkg::relay::normalize_relay_url(relay_url)
                .map_err(|error| error.to_string())?,
        })
    }

    /// Stable opaque identifier/path suffix derived only from canonical fields.
    pub fn runtime_id(&self) -> String {
        let relay_hash = hex::encode(Sha256::digest(self.relay_url.as_bytes()));
        format!("{}__{relay_hash}", self.pubkey)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedAgentRuntimeLifecycle {
    Starting,
    Listening,
    Waking,
    Ready,
    Failed,
    Stopped,
}

/// Exclusive ownership of one exact runtime generation during a planned update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedAgentRuntimeUpdateClaim {
    /// Unique identifier of the update that owns this generation.
    pub operation_id: String,
    /// Unpredictable nonce stamped when this harness generation started.
    pub start_nonce: String,
    /// Operating-system process id captured with `start_nonce`.
    pub pid: u32,
}

#[derive(Debug)]
pub struct ManagedAgentPairRuntime {
    pub process: ManagedAgentProcess,
    pub lifecycle: ManagedAgentRuntimeLifecycle,
    pub error: Option<String>,
    /// Unpredictable identity for this exact harness generation. Lifecycle
    /// frames from prior processes are rejected even when the pair is live.
    pub start_nonce: String,
    /// Present only while one planned update owns this exact process
    /// generation. Normal stop/replacement paths must not tear it down.
    pub update_claim: Option<ManagedAgentRuntimeUpdateClaim>,
}

impl std::ops::Deref for ManagedAgentPairRuntime {
    type Target = ManagedAgentProcess;

    fn deref(&self) -> &Self::Target {
        &self.process
    }
}

impl std::ops::DerefMut for ManagedAgentPairRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.process
    }
}

impl ManagedAgentPairRuntime {
    pub fn starting(process: ManagedAgentProcess) -> Self {
        let start_nonce = process.start_nonce.clone();
        Self {
            process,
            lifecycle: ManagedAgentRuntimeLifecycle::Starting,
            error: None,
            start_nonce,
            update_claim: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormalRuntimeStartDisposition {
    AlreadyRunning,
    ReplaceExited,
}

fn validate_update_operation_id(operation_id: &str) -> Result<(), String> {
    if operation_id.is_empty()
        || operation_id.len() > 128
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("managed-agent update operation id is invalid".to_string());
    }
    Ok(())
}

fn runtime_generation(
    runtime: &ManagedAgentPairRuntime,
) -> Result<ManagedAgentRuntimeUpdateClaim, String> {
    if runtime.start_nonce.is_empty() || runtime.start_nonce != runtime.process.start_nonce {
        return Err("managed-agent runtime generation is inconsistent".to_string());
    }
    Ok(ManagedAgentRuntimeUpdateClaim {
        operation_id: String::new(),
        start_nonce: runtime.start_nonce.clone(),
        pid: runtime.child.id(),
    })
}

fn verify_runtime_claim(
    runtime: &ManagedAgentPairRuntime,
    operation_id: &str,
) -> Result<ManagedAgentRuntimeUpdateClaim, String> {
    validate_update_operation_id(operation_id)?;
    let claim = runtime
        .update_claim
        .as_ref()
        .ok_or_else(|| "managed-agent runtime is not claimed for an update".to_string())?;
    if claim.operation_id != operation_id {
        return Err("managed-agent runtime is claimed by another update".to_string());
    }
    let generation = runtime_generation(runtime)?;
    if claim.start_nonce != generation.start_nonce || claim.pid != generation.pid {
        return Err("managed-agent runtime generation changed during the update".to_string());
    }
    Ok(claim.clone())
}

/// Atomically claim all exact runtime keys for one update operation.
///
/// Every key and generation is validated before any claim is written.
pub(crate) fn claim_managed_agent_runtime_pairs(
    runtimes: &mut HashMap<ManagedAgentRuntimeKey, ManagedAgentPairRuntime>,
    keys: &[ManagedAgentRuntimeKey],
    operation_id: &str,
) -> Result<(), String> {
    validate_update_operation_id(operation_id)?;
    let mut seen = HashSet::with_capacity(keys.len());
    for key in keys {
        if !seen.insert(key) {
            return Err("managed-agent update contains a duplicate runtime key".to_string());
        }
        let runtime = runtimes
            .get(key)
            .ok_or_else(|| format!("managed-agent runtime is no longer tracked: {key:?}"))?;
        runtime_generation(runtime)?;
        if runtime.update_claim.is_some() {
            return Err("managed-agent runtime is already claimed by an update".to_string());
        }
    }
    for key in keys {
        let runtime = runtimes
            .get_mut(key)
            .ok_or_else(|| "managed-agent runtime disappeared while claiming it".to_string())?;
        let mut claim = runtime_generation(runtime)?;
        claim.operation_id = operation_id.to_string();
        runtime.update_claim = Some(claim);
    }
    Ok(())
}

/// Verify that a key still resolves to the exact generation claimed by an operation.
pub(crate) fn verify_managed_agent_runtime_pair_claim(
    runtimes: &HashMap<ManagedAgentRuntimeKey, ManagedAgentPairRuntime>,
    key: &ManagedAgentRuntimeKey,
    operation_id: &str,
) -> Result<ManagedAgentRuntimeUpdateClaim, String> {
    let runtime = runtimes
        .get(key)
        .ok_or_else(|| "managed-agent runtime disappeared during the update".to_string())?;
    verify_runtime_claim(runtime, operation_id)
}

/// Remove and return a claimed runtime only after that exact generation exits.
pub(crate) fn take_exited_claimed_managed_agent_runtime(
    runtimes: &mut HashMap<ManagedAgentRuntimeKey, ManagedAgentPairRuntime>,
    key: &ManagedAgentRuntimeKey,
    operation_id: &str,
) -> Result<Option<(ManagedAgentPairRuntime, ExitStatus)>, String> {
    let status = {
        let runtime = runtimes
            .get_mut(key)
            .ok_or_else(|| "managed-agent runtime disappeared during the update".to_string())?;
        verify_runtime_claim(runtime, operation_id)?;
        runtime
            .child
            .try_wait()
            .map_err(|error| format!("failed to inspect claimed managed-agent runtime: {error}"))?
    };
    let Some(status) = status else {
        return Ok(None);
    };
    let runtime = runtimes
        .remove(key)
        .ok_or_else(|| "managed-agent runtime disappeared while removing it".to_string())?;
    Ok(Some((runtime, status)))
}

/// Clear one operation's claim without disturbing a different operation.
pub(crate) fn clear_managed_agent_runtime_pair_claim(
    runtimes: &mut HashMap<ManagedAgentRuntimeKey, ManagedAgentPairRuntime>,
    key: &ManagedAgentRuntimeKey,
    operation_id: &str,
) -> Result<(), String> {
    clear_managed_agent_runtime_pair_claims(runtimes, std::slice::from_ref(key), operation_id)
}

/// Atomically clear an operation's claims from a set of exact runtime keys.
pub(crate) fn clear_managed_agent_runtime_pair_claims(
    runtimes: &mut HashMap<ManagedAgentRuntimeKey, ManagedAgentPairRuntime>,
    keys: &[ManagedAgentRuntimeKey],
    operation_id: &str,
) -> Result<(), String> {
    validate_update_operation_id(operation_id)?;
    let mut seen = HashSet::with_capacity(keys.len());
    for key in keys {
        if !seen.insert(key) {
            return Err("managed-agent update contains a duplicate runtime key".to_string());
        }
        if let Some(claim) = runtimes
            .get(key)
            .and_then(|runtime| runtime.update_claim.as_ref())
        {
            if claim.operation_id != operation_id {
                return Err("managed-agent runtime is claimed by another update".to_string());
            }
        }
    }
    for key in keys {
        if let Some(runtime) = runtimes.get_mut(key) {
            if runtime
                .update_claim
                .as_ref()
                .is_some_and(|claim| claim.operation_id == operation_id)
            {
                runtime.update_claim = None;
            }
        }
    }
    Ok(())
}

pub(crate) fn ensure_managed_agent_runtime_pairs_unclaimed(
    runtimes: &HashMap<ManagedAgentRuntimeKey, ManagedAgentPairRuntime>,
    keys: &[ManagedAgentRuntimeKey],
) -> Result<(), String> {
    if keys.iter().any(|key| {
        runtimes
            .get(key)
            .is_some_and(|runtime| runtime.update_claim.is_some())
    }) {
        return Err(
            "managed-agent runtime is being updated and cannot be stopped or replaced".to_string(),
        );
    }
    Ok(())
}

/// Inspect an existing pair for a normal start/reconcile request.
///
/// A live process remains an idempotent duplicate start even while claimed.
/// An exited claimed generation cannot be replaced out from under its owner.
pub(crate) fn normal_runtime_start_disposition(
    runtime: &mut ManagedAgentPairRuntime,
) -> Result<NormalRuntimeStartDisposition, String> {
    if runtime
        .child
        .try_wait()
        .map_err(|error| format!("failed to inspect running process: {error}"))?
        .is_none()
    {
        if let Some(operation_id) = runtime
            .update_claim
            .as_ref()
            .map(|claim| claim.operation_id.clone())
        {
            verify_runtime_claim(runtime, &operation_id)?;
        }
        return Ok(NormalRuntimeStartDisposition::AlreadyRunning);
    }
    if runtime.update_claim.is_some() {
        return Err("managed-agent runtime is being updated and cannot be replaced".to_string());
    }
    Ok(NormalRuntimeStartDisposition::ReplaceExited)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentRuntimeStatus {
    pub pubkey: String,
    pub relay_url: String,
    /// Exact descriptor URL echoed only by reconcile result rows so callers can
    /// correlate a canonical response without normalizing on the frontend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_relay_url: Option<String>,
    pub local_setup: bool,
    pub lifecycle: ManagedAgentRuntimeLifecycle,
    pub pid: Option<u32>,
    pub error: Option<String>,
    pub log_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentRuntimeLifecycleObserverPayload {
    pub pubkey: String,
    pub relay_url: String,
    pub start_nonce: String,
    pub lifecycle: ManagedAgentRuntimeLifecycle,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentCommunityTarget {
    pub relay_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentRuntimeReceipt {
    pub key: ManagedAgentRuntimeKey,
    pub pid: u32,
    pub desktop_instance_id: String,
    pub started_at: String,
}

#[cfg(test)]
mod update_claim_tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn key(pubkey_byte: &str, relay: &str) -> ManagedAgentRuntimeKey {
        ManagedAgentRuntimeKey::new(pubkey_byte.repeat(64), relay).expect("runtime key")
    }

    fn runtime(live: bool, nonce: &str) -> ManagedAgentPairRuntime {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new(if live { "/bin/sleep" } else { "/usr/bin/true" });
            if live {
                command.arg("30");
            }
            command
        };
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            if live {
                command.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
            } else {
                command.args(["/C", "exit 0"]);
            }
            command
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("test runtime");
        let process = ManagedAgentProcess {
            child,
            log_path: std::path::PathBuf::new(),
            spawn_config_hash: 0,
            setup_mode: false,
            adapter_availability: None,
            start_nonce: nonce.to_string(),
            #[cfg(windows)]
            job: None,
            connection_config_path: None,
            connection_generation_hash: 0,
        };
        ManagedAgentPairRuntime::starting(process)
    }

    fn reap(runtime: &mut ManagedAgentPairRuntime) {
        let _ = runtime.child.kill();
        let _ = runtime.child.wait();
    }

    fn reap_all(runtimes: &mut HashMap<ManagedAgentRuntimeKey, ManagedAgentPairRuntime>) {
        for runtime in runtimes.values_mut() {
            reap(runtime);
        }
    }

    #[test]
    fn concurrent_claim_failure_writes_no_partial_claims() {
        let first = key("a", "wss://one.example");
        let second = key("b", "wss://two.example");
        let mut runtimes = HashMap::from([
            (first.clone(), runtime(false, "first")),
            (second.clone(), runtime(false, "second")),
        ]);
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&first),
            "operation-one",
        )
        .expect("first claim");

        let error = claim_managed_agent_runtime_pairs(
            &mut runtimes,
            &[second.clone(), first.clone()],
            "operation-two",
        )
        .expect_err("second operation must lose");
        assert!(error.contains("already claimed"));
        assert!(runtimes[&second].update_claim.is_none());
        assert_eq!(
            runtimes[&first]
                .update_claim
                .as_ref()
                .expect("first claim remains")
                .operation_id,
            "operation-one"
        );
        reap_all(&mut runtimes);
    }

    #[test]
    fn verification_rejects_nonce_and_pid_generation_mismatch() {
        let runtime_key = key("a", "wss://one.example");
        let mut runtimes = HashMap::from([(runtime_key.clone(), runtime(false, "original-nonce"))]);
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&runtime_key),
            "operation-one",
        )
        .expect("claim");
        verify_managed_agent_runtime_pair_claim(&runtimes, &runtime_key, "operation-one")
            .expect("exact generation");

        runtimes.get_mut(&runtime_key).expect("runtime").start_nonce =
            "replacement-nonce".to_string();
        assert!(
            verify_managed_agent_runtime_pair_claim(&runtimes, &runtime_key, "operation-one")
                .expect_err("nonce mismatch")
                .contains("generation")
        );

        let runtime = runtimes.get_mut(&runtime_key).expect("runtime");
        runtime.start_nonce = "original-nonce".to_string();
        runtime.update_claim.as_mut().expect("claim").pid += 1;
        assert!(
            verify_managed_agent_runtime_pair_claim(&runtimes, &runtime_key, "operation-one")
                .expect_err("pid mismatch")
                .contains("generation")
        );
        reap_all(&mut runtimes);
    }

    #[test]
    fn agent_wide_stop_preflight_has_no_partial_teardown() {
        let pubkey = "a".repeat(64);
        let first = ManagedAgentRuntimeKey::new(&pubkey, "wss://one.example").expect("first key");
        let second = ManagedAgentRuntimeKey::new(&pubkey, "wss://two.example").expect("second key");
        let mut runtimes = HashMap::from([
            (first.clone(), runtime(false, "first")),
            (second.clone(), runtime(false, "second")),
        ]);
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&second),
            "operation-one",
        )
        .expect("claim");
        let keys = vec![first.clone(), second.clone()];

        assert!(ensure_managed_agent_runtime_pairs_unclaimed(&runtimes, &keys).is_err());
        assert!(runtimes.contains_key(&first));
        assert!(runtimes.contains_key(&second));
        reap_all(&mut runtimes);
    }

    #[test]
    fn normal_duplicate_start_preserves_a_live_claim() {
        let runtime_key = key("a", "wss://one.example");
        let mut runtimes = HashMap::from([(runtime_key.clone(), runtime(true, "live"))]);
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&runtime_key),
            "operation-one",
        )
        .expect("claim");
        let before = runtimes[&runtime_key].update_claim.clone().expect("claim");

        let disposition =
            normal_runtime_start_disposition(runtimes.get_mut(&runtime_key).expect("runtime"))
                .expect("duplicate start");
        assert_eq!(disposition, NormalRuntimeStartDisposition::AlreadyRunning);
        assert_eq!(
            runtimes[&runtime_key]
                .update_claim
                .as_ref()
                .expect("claim remains"),
            &before
        );
        reap(runtimes.get_mut(&runtime_key).expect("runtime"));
    }

    #[test]
    fn normal_start_refuses_to_replace_an_exited_claimed_generation() {
        let runtime_key = key("a", "wss://one.example");
        let mut runtimes = HashMap::from([(runtime_key.clone(), runtime(false, "exited"))]);
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&runtime_key),
            "operation-one",
        )
        .expect("claim");
        let mut error = None;
        for _ in 0..100 {
            match normal_runtime_start_disposition(runtimes.get_mut(&runtime_key).expect("runtime"))
            {
                Ok(NormalRuntimeStartDisposition::AlreadyRunning) => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Ok(NormalRuntimeStartDisposition::ReplaceExited) => {
                    panic!("claimed generation must not be replaced")
                }
                Err(found) => {
                    error = Some(found);
                    break;
                }
            }
        }
        assert!(error
            .expect("replacement refusal")
            .contains("being updated"));
        assert!(runtimes.contains_key(&runtime_key));
        reap_all(&mut runtimes);
    }

    #[test]
    fn clear_many_refuses_another_owner_without_partial_clear() {
        let first = key("a", "wss://one.example");
        let second = key("b", "wss://two.example");
        let mut runtimes = HashMap::from([
            (first.clone(), runtime(false, "first")),
            (second.clone(), runtime(false, "second")),
        ]);
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&first),
            "operation-one",
        )
        .expect("first claim");
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&second),
            "operation-two",
        )
        .expect("second claim");

        assert!(clear_managed_agent_runtime_pair_claims(
            &mut runtimes,
            &[first.clone(), second.clone()],
            "operation-one"
        )
        .is_err());
        assert_eq!(
            runtimes[&first]
                .update_claim
                .as_ref()
                .expect("first remains")
                .operation_id,
            "operation-one"
        );
        assert_eq!(
            runtimes[&second]
                .update_claim
                .as_ref()
                .expect("second remains")
                .operation_id,
            "operation-two"
        );
        reap_all(&mut runtimes);
    }

    #[test]
    fn exited_claimed_runtime_requires_exact_take_and_clear_is_owner_scoped() {
        let runtime_key = key("a", "wss://one.example");
        let mut runtimes =
            HashMap::from([(runtime_key.clone(), runtime(false, "exiting-generation"))]);
        claim_managed_agent_runtime_pairs(
            &mut runtimes,
            std::slice::from_ref(&runtime_key),
            "operation-one",
        )
        .expect("claim");

        assert!(clear_managed_agent_runtime_pair_claim(
            &mut runtimes,
            &runtime_key,
            "operation-two"
        )
        .is_err());
        let mut taken = None;
        for _ in 0..100 {
            taken = take_exited_claimed_managed_agent_runtime(
                &mut runtimes,
                &runtime_key,
                "operation-one",
            )
            .expect("take claimed generation");
            if taken.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let taken = taken.expect("exited claimed runtime");
        assert!(taken.1.success());
        assert!(!runtimes.contains_key(&runtime_key));
    }
}
