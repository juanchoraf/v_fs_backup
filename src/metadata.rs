fn metadata_for(
    _path: &Path,
    archive_path: String,
    kind: EntryKind,
    metadata: &fs::Metadata,
) -> Result<EntryMetadata> {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(windows)]
    use std::os::windows::fs::MetadataExt;

    Ok(EntryMetadata {
        path: archive_path,
        kind,
        len: if kind == EntryKind::File {
            metadata.len()
        } else {
            0
        },
        readonly: metadata.permissions().readonly(),
        modified: metadata.modified().ok().map(system_time_to_stamp),
        accessed: metadata.accessed().ok().map(system_time_to_stamp),
        created: metadata.created().ok().map(system_time_to_stamp),
        #[cfg(unix)]
        unix: Some(UnixMetadata {
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            rdev: metadata.rdev(),
        }),
        #[cfg(windows)]
        windows: Some(WindowsMetadata {
            file_attributes: metadata.file_attributes(),
        }),
    })
}
fn apply_metadata(path: &Path, meta: &EntryMetadata) -> Result<()> {
    #[cfg(unix)]
    {
        if let Some(unix) = meta.unix {
            try_chown(path, unix.uid, unix.gid)?;
        }
    }

    if let (Some(accessed), Some(modified)) = (meta.accessed, meta.modified) {
        let times = fs::FileTimes::new()
            .set_accessed(stamp_to_system_time(accessed))
            .set_modified(stamp_to_system_time(modified));
        if let Ok(file) = OpenOptions::new().read(true).write(true).open(path) {
            file.set_times(times)
                .with_context(|| format!("failed to restore timestamps for {}", path.display()))?;
        }
    }

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to read restored metadata for {}", path.display()))?
        .permissions();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(unix) = meta.unix {
            permissions.set_mode(unix.mode);
        }
    }
    permissions.set_readonly(meta.readonly);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to restore permissions for {}", path.display()))?;

    Ok(())
}

#[cfg(unix)]
fn try_chown(path: &Path, uid: u32, gid: u32) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("path contains interior NUL: {}", path.display()))?;
    let result = unsafe { libc::chown(c_path.as_ptr(), uid, gid) };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    let ownership_restore_is_best_effort = error.kind() == io::ErrorKind::PermissionDenied
        || matches!(error.raw_os_error(), Some(code)
            if code == libc::EINVAL
                || code == libc::ENOTSUP
                || code == libc::EOPNOTSUPP
                || code == libc::ENOSYS);
    if ownership_restore_is_best_effort {
        return Ok(());
    }
    Err(error).with_context(|| format!("failed to restore owner for {}", path.display()))
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path, _target_is_dir: Option<bool>) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path, target_is_dir: Option<bool>) -> Result<()> {
    if target_is_dir.unwrap_or(false) {
        std::os::windows::fs::symlink_dir(target, link)?;
    } else {
        std::os::windows::fs::symlink_file(target, link)?;
    }
    Ok(())
}
fn system_time_to_stamp(time: SystemTime) -> FileStamp {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => FileStamp {
            seconds: duration.as_secs() as i64,
            nanos: duration.subsec_nanos(),
        },
        Err(error) => {
            let duration = error.duration();
            FileStamp {
                seconds: -(duration.as_secs() as i64),
                nanos: duration.subsec_nanos(),
            }
        }
    }
}

fn stamp_to_system_time(stamp: FileStamp) -> SystemTime {
    if stamp.seconds >= 0 {
        UNIX_EPOCH + Duration::new(stamp.seconds as u64, stamp.nanos)
    } else {
        UNIX_EPOCH - Duration::new(stamp.seconds.unsigned_abs(), stamp.nanos)
    }
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
