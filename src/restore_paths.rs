fn resolve_backup_output_path(to: &Path) -> Result<PathBuf> {
    let text = to.to_string_lossy();
    let looks_like_directory = text.ends_with('/') || text.ends_with('\\') || to.is_dir();
    if looks_like_directory {
        fs::create_dir_all(to)
            .with_context(|| format!("failed to create archive directory {}", to.display()))?;
        Ok(to.join(format!("v_fs_backup-{}.fsb", now_unix_seconds())))
    } else {
        Ok(ensure_fsb_extension(to))
    }
}

fn ensure_fsb_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
    {
        return path.to_path_buf();
    }

    let mut path = path.as_os_str().to_os_string();
    path.push(".fsb");
    PathBuf::from(path)
}

fn safe_restore_path(root: &Path, archive_path: &str) -> Result<PathBuf> {
    let archive_path = archive_path.replace('\\', "/");
    if archive_path.is_empty()
        || archive_path.starts_with('/')
        || archive_path.contains('\0')
        || archive_path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("unsafe archive path: {archive_path:?}");
    }
    Ok(archive_path
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

fn create_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    }
    Ok(())
}

fn prepare_restore_target(path: &Path, overwrite: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !overwrite {
                bail!("restore target already exists: {}", path.display());
            }
            let file_type = metadata.file_type();
            if file_type.is_dir() && !file_type.is_symlink() {
                bail!(
                    "restore target is an existing directory and will not be replaced: {}",
                    path.display()
                );
            }
            fs::remove_file(path)
                .with_context(|| format!("failed to remove existing {}", path.display()))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {}", path.display()));
        }
    }
    Ok(())
}
