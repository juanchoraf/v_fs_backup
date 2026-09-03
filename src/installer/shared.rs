use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Context, Result, simple_error};

pub fn current_exe() -> Result<PathBuf> {
    env::current_exe().context("failed to locate the current v_fs_backup executable")
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => paths_equal(&left, &right),
        _ => paths_equal(left, right),
    }
}

pub fn copy_binary(source: &Path, destination: &Path) -> Result<()> {
    if same_path(source, destination) {
        set_executable(destination)?;
        return Ok(());
    }

    let parent = destination.parent().ok_or_else(|| {
        simple_error(format!("{} has no parent directory", destination.display()))
    })?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let temporary = destination.with_file_name(format!(
        ".{}.installing.{}",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("v_fs_backup"),
        std::process::id()
    ));
    fs::copy(source, &temporary).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            temporary.display()
        )
    })?;
    set_executable(&temporary)?;
    fs::rename(&temporary, destination).with_context(|| {
        format!(
            "failed to move {} to {}",
            temporary.display(),
            destination.display()
        )
    })?;
    set_executable(destination)
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

pub fn write_text(path: &Path, text: &str) -> Result<()> {
    write_bytes(path, text.as_bytes())
}

pub fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(simple_error(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub fn remove_dir_if_exists(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(simple_error(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

pub fn command_available(name: &str) -> bool {
    find_program(name).is_some()
}

pub fn run_optional_command(command: &str, args: &[&str]) {
    if !command_available(command) {
        return;
    }
    let _ = Command::new(command).args(args).status();
}

pub fn quote_sh_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to read permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set executable permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(path: &Path) -> Result<()> {
    let _ = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;
    Ok(())
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn find_program(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.is_absolute() && candidate.is_file() {
        return Some(candidate.to_path_buf());
    }

    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}
