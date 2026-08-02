//! Validation and isolated runtime materialization for portable template skills.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Write as _,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use tauri::AppHandle;

use super::{
    known_skill_dirs, managed_agents_base_dir, AgentSkill, AgentSkillFile, ManagedAgentRecord,
    ManagedAgentRuntimeKey,
};

pub const MAX_TEMPLATE_SKILLS: usize = 16;
pub const MAX_SKILL_FILES: usize = 32;
pub const MAX_SKILL_FILE_BYTES: usize = 64 * 1024;
pub const MAX_SKILL_TOTAL_BYTES: usize = 128 * 1024;
const MAX_SKILL_NAME_BYTES: usize = 64;
const MAX_SKILL_DESCRIPTION_BYTES: usize = 512;
const MAX_SKILL_PATH_BYTES: usize = 240;
const BUILTIN_SKILL_NAME: &str = "buzz-cli";

#[derive(Debug)]
pub struct IsolatedAgentRuntime {
    pub home: PathBuf,
    pub workspace: PathBuf,
}

pub fn effective_agent_skills(record: &ManagedAgentRecord) -> &[AgentSkill] {
    record
        .skill_overrides
        .as_deref()
        .unwrap_or(&record.pinned_skills)
}

/// Start from a minimal safe parent environment, then point every conventional
/// home/config location at this agent's private runtime root. Explicit Buzz
/// and user-approved agent environment values are layered afterwards.
pub fn configure_isolated_process_environment(
    command: &mut std::process::Command,
    runtime: &IsolatedAgentRuntime,
) {
    command.env_clear();
    for key in [
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TZ",
        "TMPDIR",
        "TMP",
        "TEMP",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env("HOME", &runtime.home);
    command.env("USERPROFILE", &runtime.home);
    command.env("AGENT_CWD", &runtime.workspace);
    command.env("XDG_CONFIG_HOME", runtime.home.join(".config"));
    command.env("XDG_DATA_HOME", runtime.home.join(".local/share"));
    command.env("XDG_CACHE_HOME", runtime.home.join(".cache"));
    command.env("XDG_STATE_HOME", runtime.home.join(".local/state"));
}

#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

/// Validate the complete portable Skills section before it reaches storage,
/// public events, snapshots, or the filesystem.
pub fn validate_agent_skills(skills: &[AgentSkill]) -> Result<(), String> {
    if skills.len() > MAX_TEMPLATE_SKILLS {
        return Err(format!(
            "An agent template can include at most {MAX_TEMPLATE_SKILLS} skills."
        ));
    }

    let mut names = HashSet::new();
    let mut total_bytes = 0usize;
    for skill in skills {
        validate_skill_name(&skill.name)?;
        if skill.name == BUILTIN_SKILL_NAME {
            return Err(format!(
                "Skill name {BUILTIN_SKILL_NAME:?} is reserved by Buzz."
            ));
        }
        if !names.insert(skill.name.as_str()) {
            return Err(format!("Skill {:?} appears more than once.", skill.name));
        }
        let description = skill.description.trim();
        if description.is_empty() || description.len() > MAX_SKILL_DESCRIPTION_BYTES {
            return Err(format!(
                "Skill {:?} needs a description no longer than {MAX_SKILL_DESCRIPTION_BYTES} bytes.",
                skill.name
            ));
        }
        if skill.files.is_empty() || skill.files.len() > MAX_SKILL_FILES {
            return Err(format!(
                "Skill {:?} must include 1 to {MAX_SKILL_FILES} files.",
                skill.name
            ));
        }

        let mut paths = HashSet::new();
        let mut skill_md = None;
        for file in &skill.files {
            validate_skill_file_path(&file.path)?;
            if !paths.insert(file.path.as_str()) {
                return Err(format!(
                    "Skill {:?} contains duplicate path {:?}.",
                    skill.name, file.path
                ));
            }
            if file.content.len() > MAX_SKILL_FILE_BYTES {
                return Err(format!(
                    "{} in skill {:?} exceeds {MAX_SKILL_FILE_BYTES} bytes.",
                    file.path, skill.name
                ));
            }
            total_bytes = total_bytes
                .checked_add(file.path.len())
                .and_then(|value| value.checked_add(file.content.len()))
                .ok_or_else(|| "Skill content size overflowed.".to_string())?;
            if total_bytes > MAX_SKILL_TOTAL_BYTES {
                return Err(format!(
                    "Skills can contain at most {MAX_SKILL_TOTAL_BYTES} bytes in total."
                ));
            }
            reject_obvious_secret(&skill.name, file)?;
            if file.path == "SKILL.md" {
                skill_md = Some(file);
            }
        }
        let skill_md =
            skill_md.ok_or_else(|| format!("Skill {:?} is missing SKILL.md.", skill.name))?;
        validate_skill_frontmatter(skill, &skill_md.content)?;
    }
    Ok(())
}

fn validate_skill_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= MAX_SKILL_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && name
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Skill name {name:?} must be a lowercase slug using letters, numbers, and hyphens (maximum {MAX_SKILL_NAME_BYTES} bytes)."
        ))
    }
}

fn validate_skill_file_path(raw: &str) -> Result<(), String> {
    if raw.is_empty()
        || raw.len() > MAX_SKILL_PATH_BYTES
        || raw.contains('\\')
        || raw.split('/').any(|part| part.is_empty())
    {
        return Err(format!(
            "Skill file path {raw:?} is not a safe relative path."
        ));
    }
    let path = Path::new(raw);
    if path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        Ok(())
    } else {
        Err(format!(
            "Skill file path {raw:?} is not a safe relative path."
        ))
    }
}

fn validate_skill_frontmatter(skill: &AgentSkill, content: &str) -> Result<(), String> {
    let normalized = content.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| format!("{} SKILL.md must start with YAML frontmatter.", skill.name))?;
    let closing = rest
        .find("\n---\n")
        .ok_or_else(|| format!("{} SKILL.md has incomplete YAML frontmatter.", skill.name))?;
    let metadata: SkillFrontmatter = serde_yaml::from_str(&rest[..closing])
        .map_err(|error| format!("{} SKILL.md frontmatter is invalid: {error}", skill.name))?;
    if metadata.name.trim() != skill.name {
        return Err(format!(
            "{} SKILL.md name must match the skill name.",
            skill.name
        ));
    }
    if metadata.description.trim().is_empty() {
        return Err(format!(
            "{} SKILL.md needs a frontmatter description.",
            skill.name
        ));
    }
    if metadata.description.trim() != skill.description.trim() {
        return Err(format!(
            "{} SKILL.md description must match the skill description.",
            skill.name
        ));
    }
    Ok(())
}

fn reject_obvious_secret(skill_name: &str, file: &AgentSkillFile) -> Result<(), String> {
    let content = file.content.as_str();
    let uppercase = content.to_ascii_uppercase();
    if uppercase.contains("-----BEGIN PRIVATE KEY-----")
        || uppercase.contains("-----BEGIN RSA PRIVATE KEY-----")
        || uppercase.contains("-----BEGIN OPENSSH PRIVATE KEY-----")
    {
        return secret_error(skill_name, &file.path);
    }

    for prefix in [
        "nsec1",
        "sk_live_",
        "sk-prod-",
        "ghp_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "AKIA",
    ] {
        if contains_probable_token(content, prefix) {
            return secret_error(skill_name, &file.path);
        }
    }

    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches("export ").trim();
        let Some((key, value)) = trimmed.split_once('=').or_else(|| trimmed.split_once(':')) else {
            continue;
        };
        let key = key
            .trim_matches(|character: char| {
                character == '"' || character == '\'' || character.is_whitespace()
            })
            .to_ascii_uppercase();
        if !["API_KEY", "TOKEN", "PASSWORD", "SECRET", "PRIVATE_KEY"]
            .iter()
            .any(|marker| key.contains(marker))
        {
            continue;
        }
        let value = value
            .trim()
            .trim_matches(|character| character == '"' || character == '\'');
        if value.len() >= 8 && !looks_like_placeholder(value) {
            return secret_error(skill_name, &file.path);
        }
    }
    Ok(())
}

fn contains_probable_token(content: &str, prefix: &str) -> bool {
    let mut remaining = content;
    while let Some(index) = remaining.find(prefix) {
        let candidate: String = remaining[index..]
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':')
            })
            .take(160)
            .collect();
        if candidate.len() >= prefix.len() + 12 && !looks_like_placeholder(&candidate) {
            return true;
        }
        remaining = &remaining[index + prefix.len()..];
    }
    false
}

fn looks_like_placeholder(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    [
        "example",
        "placeholder",
        "your-",
        "your_",
        "replace",
        "xxxx",
        "<",
        "${",
        "test-only",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn secret_error(skill_name: &str, path: &str) -> Result<(), String> {
    Err(format!(
        "{path} in skill {skill_name:?} appears to contain a secret. Remove credentials and use a Project Connection or agent environment setting instead."
    ))
}

/// Stable SHA-256 of a canonical skill bundle. Order-only changes do not mint
/// a new content tree.
pub fn skill_bundle_hash(skills: &[AgentSkill]) -> Result<String, String> {
    validate_agent_skills(skills)?;
    let mut canonical = skills.to_vec();
    canonical.sort_by(|left, right| left.name.cmp(&right.name));
    for skill in &mut canonical {
        skill
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
    }
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| format!("Could not serialize template skills: {error}"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

/// Materialize a pinned skill revision into an owner-only content-addressed
/// tree, then install a private copy into this runtime pair's isolated home
/// and working directory.
pub fn materialize_isolated_agent_runtime(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
    skills: &[AgentSkill],
) -> Result<IsolatedAgentRuntime, String> {
    let bundle_hash = skill_bundle_hash(skills)?;
    let base = managed_agents_base_dir(app)?;
    let store_root = base.join("skill-trees");
    create_owner_dir(&store_root)?;
    let content_tree = store_root.join(&bundle_hash);
    materialize_content_tree(&store_root, &content_tree, skills)?;

    let runtime_root = base.join("agent-runtimes").join(key.runtime_id());
    let home = runtime_root.join("home");
    let workspace = runtime_root.join("workspace");
    create_owner_dir(&home)?;
    create_owner_dir(&workspace)?;
    for directory in [
        home.join(".config"),
        home.join(".local/share"),
        home.join(".cache"),
        home.join(".local/state"),
    ] {
        create_owner_dir(&directory)?;
    }

    let canonical_skills = workspace.join(".agents").join("skills");
    install_runtime_skills(&content_tree, &canonical_skills)?;
    write_builtin_skill(&canonical_skills)?;
    install_discovery_copies(&workspace, &canonical_skills)?;
    install_discovery_copies(&home, &canonical_skills)?;
    write_bundle_marker(&workspace, &bundle_hash)?;
    Ok(IsolatedAgentRuntime { home, workspace })
}

fn materialize_content_tree(
    store_root: &Path,
    target: &Path,
    skills: &[AgentSkill],
) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(target) {
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            verify_content_tree(target, skills)?;
            enforce_owner_tree(target)?;
            return Ok(());
        }
        return Err(format!(
            "{} is not a safe skill content directory.",
            target.display()
        ));
    }

    let staging = tempfile::Builder::new()
        .prefix(".skill-tree-")
        .tempdir_in(store_root)
        .map_err(|error| format!("Could not stage template skills: {error}"))?;
    for skill in skills {
        let skill_root = staging.path().join(&skill.name);
        create_owner_dir(&skill_root)?;
        for file in &skill.files {
            write_owner_file(&skill_root.join(&file.path), file.content.as_bytes())?;
        }
    }
    enforce_owner_tree(staging.path())?;
    match fs::rename(staging.path(), target) {
        Ok(()) => {
            let _ = staging.keep();
            Ok(())
        }
        Err(_) if target.is_dir() => {
            verify_content_tree(target, skills)?;
            enforce_owner_tree(target)
        }
        Err(error) => Err(format!(
            "Could not install skill content tree {}: {error}",
            target.display()
        )),
    }
}

fn verify_content_tree(root: &Path, skills: &[AgentSkill]) -> Result<(), String> {
    let expected: BTreeMap<String, Vec<u8>> = skills
        .iter()
        .flat_map(|skill| {
            skill.files.iter().map(move |file| {
                (
                    format!("{}/{}", skill.name, file.path),
                    file.content.as_bytes().to_vec(),
                )
            })
        })
        .collect();
    let mut actual = BTreeMap::new();
    collect_content_tree(root, root, &mut actual)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} does not match its content-addressed skill bundle.",
            root.display()
        ))
    }
}

fn collect_content_tree(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    for entry in fs::read_dir(current)
        .map_err(|error| format!("Could not read {}: {error}", current.display()))?
    {
        let entry = entry.map_err(|error| format!("Could not read skill entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            return Err(format!("{} is a symlink.", entry.path().display()));
        }
        if file_type.is_dir() {
            collect_content_tree(root, &entry.path(), files)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| format!("Could not verify skill tree: {error}"))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(entry.path())
                .map_err(|error| format!("Could not read skill file: {error}"))?;
            files.insert(relative, bytes);
        } else {
            return Err(format!(
                "{} is not a regular skill file.",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn install_runtime_skills(source: &Path, target: &Path) -> Result<(), String> {
    replace_directory_with_copy(source, target)
}

fn install_discovery_copies(root: &Path, canonical: &Path) -> Result<(), String> {
    replace_directory_with_copy(canonical, &root.join(".agents").join("skills"))?;
    for relative in known_skill_dirs() {
        replace_directory_with_copy(canonical, &root.join(relative))?;
    }
    Ok(())
}

fn replace_directory_with_copy(source: &Path, target: &Path) -> Result<(), String> {
    if source == target {
        return Ok(());
    }
    if let Ok(metadata) = fs::symlink_metadata(target) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{} is a symlink; refusing to replace a managed skill directory.",
                target.display()
            ));
        }
        ensure_no_symlinks(target)?;
        fs::remove_dir_all(target)
            .map_err(|error| format!("Could not replace {}: {error}", target.display()))?;
    }
    create_owner_dir(target)?;
    copy_owner_tree(source, target)
}

fn copy_owner_tree(source: &Path, target: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source)
        .map_err(|error| format!("Could not read {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("Could not read skill entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            return Err(format!(
                "{} is a symlink; refusing to copy a skill tree.",
                entry.path().display()
            ));
        }
        let destination = target.join(entry.file_name());
        if file_type.is_dir() {
            create_owner_dir(&destination)?;
            copy_owner_tree(&entry.path(), &destination)?;
        } else if file_type.is_file() {
            let bytes = fs::read(entry.path())
                .map_err(|error| format!("Could not read skill file: {error}"))?;
            write_owner_file(&destination, &bytes)?;
        } else {
            return Err(format!(
                "{} is not a regular skill file.",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn write_builtin_skill(canonical: &Path) -> Result<(), String> {
    let root = canonical.join(BUILTIN_SKILL_NAME);
    create_owner_dir(&root)?;
    write_owner_file(
        &root.join("SKILL.md"),
        super::bundled_buzz_cli_skill().as_bytes(),
    )
}

fn write_bundle_marker(workspace: &Path, bundle_hash: &str) -> Result<(), String> {
    write_owner_file(
        &workspace.join(".buzz-template-skills"),
        format!("{bundle_hash}\n").as_bytes(),
    )
}

fn write_owner_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory.", path.display()))?;
    create_owner_dir(parent)?;
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(format!(
            "{} is a symlink; refusing to write a skill file.",
            path.display()
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not secure {}: {error}", path.display()))?;
    }
    Ok(())
}

fn create_owner_dir(path: &Path) -> Result<(), String> {
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(format!(
            "{} is a symlink; refusing to use it for agent skills.",
            path.display()
        ));
    }
    fs::create_dir_all(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not secure {}: {error}", path.display()))?;
    }
    Ok(())
}

fn ensure_no_symlinks(root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("Could not inspect {}: {error}", root.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} is a symlink.", root.display()));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(root)
            .map_err(|error| format!("Could not read {}: {error}", root.display()))?
        {
            ensure_no_symlinks(
                &entry
                    .map_err(|error| format!("Could not read skill entry: {error}"))?
                    .path(),
            )?;
        }
    }
    Ok(())
}

fn enforce_owner_tree(root: &Path) -> Result<(), String> {
    ensure_no_symlinks(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let metadata = fs::metadata(root)
            .map_err(|error| format!("Could not inspect {}: {error}", root.display()))?;
        let mode = if metadata.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(root, fs::Permissions::from_mode(mode))
            .map_err(|error| format!("Could not secure {}: {error}", root.display()))?;
        if metadata.is_dir() {
            for entry in fs::read_dir(root)
                .map_err(|error| format!("Could not read {}: {error}", root.display()))?
            {
                enforce_owner_tree(
                    &entry
                        .map_err(|error| format!("Could not read skill entry: {error}"))?
                        .path(),
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "template_skills/tests.rs"]
mod tests;
