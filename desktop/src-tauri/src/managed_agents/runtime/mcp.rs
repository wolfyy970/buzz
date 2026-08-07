//! Managed-agent MCP launch resolution and ephemeral config handoff.

use std::{
    collections::BTreeMap,
    io::Write as _,
    path::{Path, PathBuf},
};

use buzz_core_pkg::mcp_config::{ConfiguredMcpServer, McpLaunchConfigDocument};

const CONFIGURED_SERVER_NAME: &str = "agent-configured";
const CONFIGURED_SERVER_FALLBACK_NAME: &str = "agent-configured-2";

fn is_known_catalog_command(command: &str) -> bool {
    let command = command.trim();
    !command.is_empty()
        && crate::managed_agents::discovery::KNOWN_ACP_RUNTIMES
            .iter()
            .filter_map(|runtime| runtime.mcp_command)
            .any(|known| command == known)
}

#[derive(Debug)]
pub(crate) struct ResolvedMcpLaunch {
    pub legacy_command: Option<PathBuf>,
    pub structured_config: Option<Vec<u8>>,
}

/// Return the single command shown in summaries and spawn-diff receipts.
///
/// Catalog configuration remains the default. A different non-empty value on
/// an existing record is treated as a legacy machine-local compatibility
/// override until Project connections replace that storage path.
pub(crate) fn effective_mcp_command(record_command: &str, catalog_command: Option<&str>) -> String {
    let configured = record_command.trim();
    if !configured.is_empty()
        && catalog_command.is_none_or(|catalog| configured != catalog.trim())
        && !is_known_catalog_command(configured)
    {
        return configured.to_string();
    }
    catalog_command.unwrap_or_default().trim().to_string()
}

pub(crate) fn resolve_mcp_launch(
    record_command: &str,
    catalog_command: Option<&str>,
) -> Result<ResolvedMcpLaunch, String> {
    resolve_mcp_launch_with(record_command, catalog_command, super::resolve_command)
}

fn resolve_mcp_launch_with(
    record_command: &str,
    catalog_command: Option<&str>,
    resolve: impl FnMut(&str) -> Option<PathBuf>,
) -> Result<ResolvedMcpLaunch, String> {
    resolve_mcp_launch_with_validation(
        record_command,
        catalog_command,
        resolve,
        validate_resolved_command,
    )
}

fn resolve_mcp_launch_with_validation(
    record_command: &str,
    catalog_command: Option<&str>,
    mut resolve: impl FnMut(&str) -> Option<PathBuf>,
    mut validate: impl FnMut(&Path) -> Result<(), String>,
) -> Result<ResolvedMcpLaunch, String> {
    let catalog_command = catalog_command
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let legacy_command = catalog_command.and_then(&mut resolve);

    let configured = record_command.trim();
    if configured.is_empty()
        || catalog_command == Some(configured)
        || is_known_catalog_command(configured)
    {
        return Ok(ResolvedMcpLaunch {
            legacy_command,
            structured_config: None,
        });
    }

    let configured_command = resolve(configured).ok_or_else(|| {
        "The configured MCP command could not be found on this device.".to_string()
    })?;
    validate(&configured_command)?;
    if legacy_command
        .as_deref()
        .is_some_and(|legacy| same_executable(legacy, &configured_command))
    {
        return Ok(ResolvedMcpLaunch {
            legacy_command,
            structured_config: None,
        });
    }

    let server_name = configured_server_name(legacy_command.as_deref());
    let configured_command = configured_command
        .to_str()
        .ok_or_else(|| {
            "The configured MCP command path is not valid UTF-8 on this device.".to_string()
        })?
        .to_string();
    let document = McpLaunchConfigDocument::new(vec![ConfiguredMcpServer::Stdio {
        name: server_name,
        command: configured_command,
        args: Vec::new(),
        env: BTreeMap::new(),
    }]);
    let structured_config = document
        .to_json()
        .map_err(|error| format!("The configured MCP command is invalid: {error}"))?;

    Ok(ResolvedMcpLaunch {
        legacy_command,
        structured_config: Some(structured_config),
    })
}

fn validate_resolved_command(path: &Path) -> Result<(), String> {
    if !crate::managed_agents::discovery::is_executable_file(path) {
        return Err("The configured MCP command is not executable on this device.".to_string());
    }
    #[cfg(windows)]
    {
        let supported = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                ["exe", "com", "cmd", "bat"]
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            });
        if !supported {
            return Err(
                "The configured MCP command is not a supported Windows executable or shim."
                    .to_string(),
            );
        }
    }
    Ok(())
}

pub(crate) fn same_executable(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn validate_harness_compatibility(
    launch: &ResolvedMcpLaunch,
    resolved_acp_command: &Path,
) -> Result<(), String> {
    validate_harness_compatibility_with(launch, resolved_acp_command, super::resolve_command)
}

fn validate_harness_compatibility_with(
    launch: &ResolvedMcpLaunch,
    resolved_acp_command: &Path,
    mut resolve: impl FnMut(&str) -> Option<PathBuf>,
) -> Result<(), String> {
    if launch.structured_config.is_none() {
        return Ok(());
    }
    let bundled_acp_command =
        resolve(crate::managed_agents::DEFAULT_ACP_COMMAND).ok_or_else(|| {
            "Configured MCP servers require the bundled Buzz ACP harness.".to_string()
        })?;
    if !same_executable(resolved_acp_command, &bundled_acp_command) {
        return Err("Configured MCP servers require the bundled Buzz ACP harness.".to_string());
    }
    Ok(())
}

fn configured_server_name(legacy_command: Option<&Path>) -> String {
    let legacy_name = legacy_command
        .and_then(Path::file_stem)
        .and_then(|value| value.to_str());
    if legacy_name == Some(CONFIGURED_SERVER_NAME) {
        CONFIGURED_SERVER_FALLBACK_NAME.to_string()
    } else {
        CONFIGURED_SERVER_NAME.to_string()
    }
}

/// Configure the child environment and keep the structured document alive for
/// the harness process lifetime.
///
/// `TempPath` removes the file on any pre-spawn error or when the owning
/// process record is dropped. This compatibility document contains no
/// credentials, arguments, or custom environment values.
pub(crate) fn configure_mcp_environment(
    command: &mut std::process::Command,
    launch: &ResolvedMcpLaunch,
) -> Result<Option<tempfile::TempPath>, String> {
    command.env_remove("BUZZ_ACP_MCP_COMMAND");
    command.env_remove("BUZZ_ACP_MCP_CONFIG");

    if let Some(legacy_command) = launch.legacy_command.as_deref() {
        command.env("BUZZ_ACP_MCP_COMMAND", legacy_command);
    }

    let Some(bytes) = launch.structured_config.as_deref() else {
        return Ok(None);
    };
    let mut file = tempfile::Builder::new()
        .prefix("buzz-acp-mcp-")
        .suffix(".json")
        .tempfile()
        .map_err(|error| format!("failed to create the MCP launch file: {error}"))?;
    file.write_all(bytes)
        .and_then(|()| file.as_file_mut().sync_all())
        .map_err(|error| format!("failed to write the MCP launch file: {error}"))?;
    let path = file.into_temp_path();
    command.env("BUZZ_ACP_MCP_CONFIG", path.as_os_str());
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn resolved(path: &str) -> PathBuf {
        PathBuf::from(format!("/resolved/{path}"))
    }

    fn resolve_for_test(
        record_command: &str,
        catalog_command: Option<&str>,
        resolve: impl FnMut(&str) -> Option<PathBuf>,
    ) -> Result<ResolvedMcpLaunch, String> {
        resolve_mcp_launch_with_validation(record_command, catalog_command, resolve, |_| Ok(()))
    }

    #[test]
    fn catalog_default_stays_on_the_legacy_compatibility_input() {
        let launch = resolve_for_test("buzz-dev-mcp", Some("buzz-dev-mcp"), |value| {
            Some(resolved(value))
        })
        .unwrap();

        assert_eq!(
            launch.legacy_command.as_deref(),
            Some(Path::new("/resolved/buzz-dev-mcp"))
        );
        assert!(launch.structured_config.is_none());
    }

    #[test]
    fn configured_command_composes_with_the_catalog_server() {
        let launch = resolve_for_test("analytics-mcp", Some("buzz-dev-mcp"), |value| {
            Some(resolved(value))
        })
        .unwrap();
        let structured = launch.structured_config.as_deref().unwrap();
        let servers = buzz_core_pkg::mcp_config::parse_mcp_config_document(structured).unwrap();

        assert_eq!(
            launch.legacy_command.as_deref(),
            Some(Path::new("/resolved/buzz-dev-mcp"))
        );
        assert_eq!(
            servers,
            vec![ConfiguredMcpServer::Stdio {
                name: CONFIGURED_SERVER_NAME.to_string(),
                command: "/resolved/analytics-mcp".to_string(),
                args: Vec::new(),
                env: BTreeMap::new(),
            }]
        );
    }

    #[cfg(unix)]
    #[test]
    fn configured_command_rejects_a_non_executable_file() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = tempfile::tempdir().unwrap();
        let command = temp.path().join("analytics-mcp");
        fs::write(&command, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&command, fs::Permissions::from_mode(0o600)).unwrap();

        let error =
            resolve_mcp_launch_with("analytics-mcp", None, |_| Some(command.clone())).unwrap_err();

        assert_eq!(
            error,
            "The configured MCP command is not executable on this device."
        );
    }

    #[test]
    fn configured_command_rejects_a_directory() {
        let temp = tempfile::tempdir().unwrap();

        let error =
            resolve_mcp_launch_with("analytics-mcp", None, |_| Some(temp.path().to_path_buf()))
                .unwrap_err();

        assert_eq!(
            error,
            "The configured MCP command is not executable on this device."
        );
    }

    #[test]
    fn configured_command_revalidates_a_deleted_cached_path() {
        let temp = tempfile::tempdir().unwrap();
        let command = temp.path().join(if cfg!(windows) {
            "analytics-mcp.exe"
        } else {
            "analytics-mcp"
        });
        fs::write(&command, b"stub").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&command, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::remove_file(&command).unwrap();

        let error =
            resolve_mcp_launch_with("analytics-mcp", None, |_| Some(command.clone())).unwrap_err();

        assert_eq!(
            error,
            "The configured MCP command is not executable on this device."
        );
    }

    #[test]
    fn stale_catalog_default_is_not_resurrected_for_another_runtime() {
        let launch = resolve_for_test("buzz-dev-mcp", None, |_| {
            panic!("a stale catalog command must not be resolved")
        })
        .unwrap();

        assert!(launch.legacy_command.is_none());
        assert!(launch.structured_config.is_none());
        assert_eq!(effective_mcp_command("buzz-dev-mcp", None), "");
    }

    #[test]
    fn distinct_command_without_a_catalog_server_uses_only_structured_config() {
        let launch =
            resolve_for_test("analytics-mcp", None, |value| Some(resolved(value))).unwrap();
        let servers = buzz_core_pkg::mcp_config::parse_mcp_config_document(
            launch.structured_config.as_deref().unwrap(),
        )
        .unwrap();

        assert!(launch.legacy_command.is_none());
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name(), CONFIGURED_SERVER_NAME);
    }

    #[test]
    fn command_path_with_spaces_remains_one_executable() {
        let launch = resolve_for_test("analytics-mcp", None, |_| {
            Some(PathBuf::from("/Applications/Analytics Tools/server"))
        })
        .unwrap();
        let servers = buzz_core_pkg::mcp_config::parse_mcp_config_document(
            launch.structured_config.as_deref().unwrap(),
        )
        .unwrap();

        assert_eq!(
            servers,
            vec![ConfiguredMcpServer::Stdio {
                name: CONFIGURED_SERVER_NAME.to_string(),
                command: "/Applications/Analytics Tools/server".to_string(),
                args: Vec::new(),
                env: BTreeMap::new(),
            }]
        );
    }

    #[test]
    fn alternate_spelling_of_the_same_executable_is_not_launched_twice() {
        let launch = resolve_for_test("/resolved/buzz-dev-mcp", Some("buzz-dev-mcp"), |_| {
            Some(resolved("buzz-dev-mcp"))
        })
        .unwrap();

        assert!(launch.structured_config.is_none());
    }

    #[test]
    fn structured_server_name_does_not_collide_with_the_legacy_server() {
        let launch = resolve_for_test("analytics-mcp", Some("agent-configured"), |value| {
            Some(resolved(value))
        })
        .unwrap();
        let servers = buzz_core_pkg::mcp_config::parse_mcp_config_document(
            launch.structured_config.as_deref().unwrap(),
        )
        .unwrap();

        assert_eq!(servers[0].name(), CONFIGURED_SERVER_FALLBACK_NAME);
    }

    #[test]
    fn missing_configured_command_fails_instead_of_silently_dropping_tools() {
        let error = resolve_for_test("missing-mcp", Some("buzz-dev-mcp"), |value| {
            (value == "buzz-dev-mcp").then(|| resolved(value))
        })
        .err()
        .unwrap();

        assert_eq!(
            error,
            "The configured MCP command could not be found on this device."
        );
        assert!(!error.contains("missing-mcp"));
    }

    #[test]
    fn structured_config_fails_closed_for_a_custom_acp_harness() {
        let launch =
            resolve_for_test("analytics-mcp", None, |value| Some(resolved(value))).unwrap();

        let error = validate_harness_compatibility_with(&launch, Path::new("/custom/acp"), |_| {
            Some(resolved("buzz-acp"))
        })
        .unwrap_err();

        assert_eq!(
            error,
            "Configured MCP servers require the bundled Buzz ACP harness."
        );
        assert!(!error.contains("/custom/acp"));
    }

    #[test]
    fn legacy_only_config_does_not_constrain_the_acp_harness() {
        let launch = resolve_for_test("buzz-dev-mcp", Some("buzz-dev-mcp"), |value| {
            Some(resolved(value))
        })
        .unwrap();

        validate_harness_compatibility_with(&launch, Path::new("/custom/acp"), |_| {
            panic!("legacy-only configuration must not resolve the bundled harness")
        })
        .unwrap();
    }

    #[test]
    fn ephemeral_document_is_owner_only_and_removed_with_its_guard() {
        let launch = resolve_for_test("analytics-mcp", Some("buzz-dev-mcp"), |value| {
            Some(resolved(value))
        })
        .unwrap();
        let mut command = std::process::Command::new("unused");
        let guard = configure_mcp_environment(&mut command, &launch)
            .unwrap()
            .unwrap();
        let path = guard.to_path_buf();
        let env: BTreeMap<_, _> = command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect();

        assert!(path.is_absolute());
        assert_eq!(
            env.get("BUZZ_ACP_MCP_COMMAND"),
            Some(&"/resolved/buzz-dev-mcp".to_string())
        );
        assert_eq!(
            env.get("BUZZ_ACP_MCP_CONFIG"),
            Some(&path.to_string_lossy().into_owned())
        );
        assert_eq!(
            buzz_core_pkg::mcp_config::parse_mcp_config_document(&std::fs::read(&path).unwrap())
                .unwrap()
                .len(),
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
                0
            );
        }

        drop(guard);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn spawned_harness_can_read_document_until_process_record_is_dropped() {
        use std::process::Stdio;

        let launch = resolve_for_test("analytics-mcp", Some("buzz-dev-mcp"), |value| {
            Some(resolved(value))
        })
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let captured = temp.path().join("captured.json");
        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", "cat \"$BUZZ_ACP_MCP_CONFIG\" > \"$CAPTURED\""])
            .env("CAPTURED", &captured)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let guard = configure_mcp_environment(&mut command, &launch)
            .unwrap()
            .unwrap();
        let config_path = guard.to_path_buf();
        let child = command.spawn().unwrap();
        let record: crate::managed_agents::ManagedAgentRecord =
            serde_json::from_value(serde_json::json!({
                "pubkey": "cc".repeat(32),
                "name": "test",
                "private_key_nsec": "nsec1fake",
                "relay_url": "",
                "acp_command": "buzz-acp",
                "agent_command": "goose",
                "agent_args": [],
                "mcp_command": "",
                "turn_timeout_seconds": 320,
                "system_prompt": null,
                "model": null,
                "provider": null,
                "env_vars": {},
                "created_at": "now",
                "updated_at": "now"
            }))
            .unwrap();
        let mut process = crate::managed_agents::ManagedAgentProcess {
            child,
            log_path: PathBuf::new(),
            spawn_config: crate::managed_agents::spawn_snapshot::prospective_spawn_config_snapshot(
                &record,
                &[],
                &[],
                "wss://relay.example",
                &Default::default(),
            ),
            setup_mode: false,
            adapter_availability: None,
            start_nonce: "test-nonce".to_string(),
            _mcp_config_path: Some(guard),
        };

        assert!(process.child.wait().unwrap().success());
        assert!(config_path.exists());
        assert_eq!(
            fs::read(&captured).unwrap(),
            launch.structured_config.unwrap()
        );

        drop(process);
        assert!(!config_path.exists());
    }

    #[test]
    fn summary_value_prefers_only_a_distinct_record_compatibility_command() {
        assert_eq!(
            effective_mcp_command("", Some("buzz-dev-mcp")),
            "buzz-dev-mcp"
        );
        assert_eq!(
            effective_mcp_command("buzz-dev-mcp", Some("buzz-dev-mcp")),
            "buzz-dev-mcp"
        );
        assert_eq!(
            effective_mcp_command("analytics-mcp", Some("buzz-dev-mcp")),
            "analytics-mcp"
        );
    }

    #[test]
    fn ambient_mcp_environment_is_scrubbed_when_no_server_is_configured() {
        let launch = ResolvedMcpLaunch {
            legacy_command: None,
            structured_config: None,
        };
        let mut command = std::process::Command::new("unused");
        command.env("BUZZ_ACP_MCP_COMMAND", "ambient");
        command.env("BUZZ_ACP_MCP_CONFIG", "/ambient/config.json");

        let guard = configure_mcp_environment(&mut command, &launch).unwrap();

        assert!(guard.is_none());
        let env: BTreeMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned)))
            .collect();
        assert_eq!(
            env.get(std::ffi::OsStr::new("BUZZ_ACP_MCP_COMMAND")),
            Some(&None)
        );
        assert_eq!(
            env.get(std::ffi::OsStr::new("BUZZ_ACP_MCP_CONFIG")),
            Some(&None)
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_command_path_is_rejected() {
        use std::os::unix::ffi::OsStringExt as _;

        let error = resolve_for_test("analytics-mcp", None, |_| {
            Some(PathBuf::from(std::ffi::OsString::from_vec(vec![0xff])))
        })
        .err()
        .unwrap();

        assert_eq!(
            error,
            "The configured MCP command path is not valid UTF-8 on this device."
        );
    }
}
