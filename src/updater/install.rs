use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::{Context, Result, simple_error};
use v_concat::v_concat;

use super::{APP_NAME, prepare_temp_dir};

pub(super) fn install_downloaded_asset(path: &Path, asset_name: &str) -> Result<String> {
    if cfg!(windows) {
        return install_windows_asset(path, asset_name);
    }

    if asset_name.ends_with(".deb") {
        return install_deb(path);
    }
    if asset_name.ends_with(".pkg") {
        return install_pkg(path);
    }
    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".zip") {
        return install_unix_archive(path, asset_name);
    }
    if is_raw_binary_asset(asset_name) {
        return install_binary_over_current(path);
    }

    Err(simple_error(v_concat!(
        "downloaded {asset_name}, but this platform does not know how to install that asset type"
    )))
}

fn is_raw_binary_asset(asset_name: &str) -> bool {
    !asset_name.contains('.') && asset_name.starts_with(APP_NAME)
}

fn install_windows_asset(path: &Path, asset_name: &str) -> Result<String> {
    if asset_name.ends_with(".exe") {
        Command::new(path)
            .spawn()
            .with_context(|| format!("failed to start Windows installer '{}'", path.display()))?;
        return Ok("Started the Windows EXE installer. Complete it to finish updating.".to_owned());
    }

    if asset_name.ends_with(".msi") {
        Command::new("msiexec")
            .arg("/i")
            .arg(path)
            .spawn()
            .with_context(|| format!("failed to start msiexec for '{}'", path.display()))?;
        return Ok("Started msiexec. Complete it to finish updating.".to_owned());
    }

    Err(simple_error(v_concat!(
        "downloaded {asset_name}, but Windows updates require the .exe or .msi installer asset"
    )))
}

fn install_deb(path: &Path) -> Result<String> {
    let package = path.as_os_str().to_os_string();
    if command_available("apt") {
        run_privileged(
            "apt",
            vec![OsString::from("install"), OsString::from("-y"), package],
        )?;
        return Ok("Installed the Debian package with apt.".to_owned());
    }
    if command_available("apt-get") {
        run_privileged(
            "apt-get",
            vec![
                OsString::from("install"),
                OsString::from("-y"),
                path.as_os_str().to_os_string(),
            ],
        )?;
        return Ok("Installed the Debian package with apt-get.".to_owned());
    }
    if command_available("dpkg") {
        run_privileged(
            "dpkg",
            vec![OsString::from("-i"), path.as_os_str().to_os_string()],
        )?;
        return Ok("Installed the Debian package with dpkg.".to_owned());
    }

    Err(simple_error(
        "downloaded a .deb update, but apt, apt-get, and dpkg were not found",
    ))
}

fn install_pkg(path: &Path) -> Result<String> {
    run_privileged(
        "installer",
        vec![
            OsString::from("-pkg"),
            path.as_os_str().to_os_string(),
            OsString::from("-target"),
            OsString::from("/"),
        ],
    )?;
    Ok("Installed the macOS package with installer.".to_owned())
}

fn install_unix_archive(path: &Path, asset_name: &str) -> Result<String> {
    let extract_dir = prepare_temp_dir("extract")?;
    if asset_name.ends_with(".tar.gz") {
        run_command(
            Command::new("tar")
                .arg("-xzf")
                .arg(path)
                .arg("-C")
                .arg(&extract_dir),
            "extract update archive",
        )?;
    } else {
        run_command(
            Command::new("unzip")
                .arg("-q")
                .arg(path)
                .arg("-d")
                .arg(&extract_dir),
            "extract update archive",
        )?;
    }

    let new_binary = extract_dir.join(APP_NAME).join("bin").join(APP_NAME);
    if !new_binary.is_file() {
        return Err(simple_error(v_concat!(
            "update archive did not contain '{}'",
            new_binary.display()
        )));
    }

    install_binary_over_current(&new_binary)
}

fn install_binary_over_current(new_binary: &Path) -> Result<String> {
    let current = env::current_exe().context("failed to resolve current executable path")?;

    if let Some(parent) = current.parent() {
        let staged = parent.join(v_concat!(".{APP_NAME}_update_{}", std::process::id()));
        if fs::copy(new_binary, &staged).is_ok() {
            set_executable(&staged)?;
            match fs::rename(&staged, &current) {
                Ok(()) => {
                    return Ok(v_concat!(
                        "Replaced current binary at {}",
                        current.display()
                    ));
                }
                Err(_) => {
                    let _ = fs::remove_file(&staged);
                }
            }
        }
    }

    run_privileged(
        "install",
        vec![
            OsString::from("-m"),
            OsString::from("0755"),
            new_binary.as_os_str().to_os_string(),
            current.as_os_str().to_os_string(),
        ],
    )?;
    Ok(v_concat!(
        "Replaced current binary at {}",
        current.display()
    ))
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(not(unix))]
    let _ = path;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("failed to mark '{}' executable", path.display()))?;
    }

    Ok(())
}

fn run_privileged(program: &str, args: Vec<OsString>) -> Result<()> {
    if cfg!(windows) || is_root() {
        let mut command = Command::new(program);
        command.args(&args);
        return run_command(&mut command, program);
    }

    if !command_available("sudo") {
        return Err(simple_error(v_concat!(
            "{program} needs elevated permissions; rerun as root or install sudo"
        )));
    }

    let mut command = Command::new("sudo");
    command.arg(program).args(&args);
    run_command(&mut command, program)
}

fn is_root() -> bool {
    let Ok(output) = Command::new("id").arg("-u").output() else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "0"
}

pub(super) fn command_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub(super) fn run_command(command: &mut Command, description: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("failed to run {description}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(simple_error(v_concat!(
            "{description} exited with status {status}"
        )))
    }
}
