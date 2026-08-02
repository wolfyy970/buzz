use std::{
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{QuarantineReason, RecoveryJournalEntry, JOURNAL_MAX_BYTES};

pub(super) fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn read_entries(path: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| format!("Could not read the private update journal: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Could not inspect the private update journal: {error}"))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

pub(super) fn ensure_private_anchor(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("The update journal path is not a private directory.".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_directory(path)?;
        }
        Err(error) => {
            return Err(format!(
                "Could not inspect the private update journal: {error}"
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not protect the private update journal: {error}"))?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("Could not verify the private update journal: {error}"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o777 != 0o700
        {
            return Err("The update journal directory is not owner-only.".to_string());
        }
    }
    Ok(())
}

pub(super) fn ensure_private_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("The update journal path is not a private directory.".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_directory(path)?;
        }
        Err(error) => {
            return Err(format!(
                "Could not inspect the private update journal: {error}"
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not protect the private update journal: {error}"))?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("Could not verify the private update journal: {error}"))?;
        let expected_uid = path
            .parent()
            .ok_or_else(|| "The update journal has no private parent.".to_string())?
            .metadata()
            .map_err(|error| format!("Could not verify the update journal owner: {error}"))?
            .uid();
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.uid() != expected_uid
            || metadata.permissions().mode() & 0o777 != 0o700
        {
            return Err("The update journal directory is not owner-only.".to_string());
        }
    }
    Ok(())
}

pub(super) fn create_private_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
            .create(path)
            .map_err(|error| format!("Could not create the private update journal: {error}"))
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
            .map_err(|error| format!("Could not create the private update journal: {error}"))
    }
}

pub(super) fn open_owner_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
        .open(path)
        .map_err(|error| format!("Could not open the private update transaction: {error}"))
}

pub(super) fn read_owner_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let file = open_owner_file(path)?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Could not inspect the private update transaction: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
        return Err("The update transaction file has an invalid type or size.".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let expected_uid = path
            .parent()
            .ok_or_else(|| "The update transaction has no private parent.".to_string())?
            .metadata()
            .map_err(|error| format!("Could not verify the update transaction owner: {error}"))?
            .uid();
        if metadata.uid() != expected_uid || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err("The update transaction file is not owner-only.".to_string());
        }
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read the private update transaction: {error}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err("The update transaction file is too large.".to_string());
    }
    Ok(bytes)
}

pub(super) fn atomic_write_owner_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "The update transaction has no private parent.".to_string())?;
    ensure_private_directory(parent)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("The update transaction path is not a regular file.".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Could not inspect the update transaction path: {error}"
            ));
        }
    }
    #[cfg(windows)]
    {
        use atomic_write_file::AtomicWriteFile;

        let mut file = AtomicWriteFile::open(path)
            .map_err(|error| format!("Could not create the update transaction write: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("Could not write the update transaction: {error}"))?;
        file.commit()
            .map_err(|error| format!("Could not commit the update transaction: {error}"))?;
        sync_directory(parent)
    }
    #[cfg(not(windows))]
    {
        let temporary = parent.join(format!(".write-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(|error| {
                format!("Could not create the update transaction write: {error}")
            })?;
            file.write_all(bytes)
                .map_err(|error| format!("Could not write the update transaction: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("Could not sync the update transaction: {error}"))?;
            fs::rename(&temporary, path)
                .map_err(|error| format!("Could not commit the update transaction: {error}"))?;
            sync_directory(parent)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

pub(super) fn atomic_create_owner_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "The update transaction has no private parent.".to_string())?;
    ensure_private_directory(parent)?;
    let temporary = parent.join(format!(".create-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        write_new_owner_file(&temporary, bytes)?;
        fs::hard_link(&temporary, path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "An update transaction with this id already exists.".to_string()
            } else {
                format!("Could not publish the update transaction: {error}")
            }
        })?;
        sync_directory(parent)?;
        fs::remove_file(&temporary)
            .map_err(|error| format!("Could not clear the transaction write: {error}"))?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn write_new_owner_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("Could not create the update transaction write: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("Could not write the update transaction: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("Could not sync the update transaction: {error}"))
}

pub(super) fn remove_regular_owner_file(path: &Path) -> Result<(), String> {
    let _ = read_owner_file(path, JOURNAL_MAX_BYTES)?;
    fs::remove_file(path)
        .map_err(|error| format!("Could not remove the update transaction: {error}"))
}

pub(super) fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("Could not sync the private update journal: {error}"))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn classify_invalid_path(path: &Path) -> QuarantineReason {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return QuarantineReason::InvalidDocument,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return QuarantineReason::LinkOrNonFile;
    }
    if metadata.len() == 0 || metadata.len() > JOURNAL_MAX_BYTES {
        return QuarantineReason::Oversized;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let expected_uid = path
            .parent()
            .and_then(|parent| parent.metadata().ok())
            .map(|parent| parent.uid());
        if expected_uid != Some(metadata.uid()) || metadata.permissions().mode() & 0o777 != 0o600 {
            return QuarantineReason::InvalidPermissions;
        }
    }
    QuarantineReason::InvalidDocument
}

pub(super) fn recovery_entry_name(entry: &RecoveryJournalEntry) -> &str {
    match entry {
        RecoveryJournalEntry::Transaction(transaction, _) => &transaction.transaction_id,
        RecoveryJournalEntry::Quarantine(quarantined, _) => &quarantined.source_name,
    }
}
