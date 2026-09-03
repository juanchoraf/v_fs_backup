use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Context, Result, simple_error};
use v_concat::v_concat;

use super::APP_NAME;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(super) fn install_downloaded_asset(path: &Path, asset_name: &str) -> Result<String> {
    let lower = asset_name.to_ascii_lowercase();

    if lower.ends_with(".deb") {
        install_deb(path)?;
        return Ok(
            "Installed the Linux package. Open a new terminal and run v_fs_backup.".to_owned(),
        );
    }
    if lower.ends_with(".pkg") {
        install_pkg(path)?;
        return Ok(
            "Installed the macOS package. Open /Applications/v_fs_backup.app or run v_fs_backup."
                .to_owned(),
        );
    }
    if lower.ends_with(".msi") {
        install_msi(path)?;
        return Ok(
            "Installed the Windows MSI. Close and open v_fs_backup again to apply it.".to_owned(),
        );
    }
    if lower.ends_with(".exe") {
        install_exe(path)?;
        return Ok(
            "Installed the Windows EXE package. Close and open v_fs_backup again to apply it."
                .to_owned(),
        );
    }
    if lower.ends_with(".tar.gz") || lower.ends_with(".zip") {
        install_archive(path, &lower)?;
        return Ok("Installed the portable archive update over the current binary.".to_owned());
    }
    if is_raw_binary_asset(asset_name) {
        install_binary_over_current(path)?;
        return Ok("Installed the raw binary update over the current executable.".to_owned());
    }

    Err(simple_error(v_concat!(
        "downloaded {asset_name}, but this platform does not know how to install that asset type"
    )))
}

fn install_deb(path: &Path) -> Result<()> {
    if cfg!(not(target_os = "linux")) {
        return Err(simple_error(
            "Debian packages can only be installed on Linux",
        ));
    }

    if is_root_user() {
        run_command(
            Command::new("dpkg").arg("-i").arg(path),
            "install Debian package",
        )
    } else if command_available("pkexec") {
        run_command(
            Command::new("pkexec").arg("dpkg").arg("-i").arg(path),
            "install Debian package with pkexec",
        )
    } else if command_available("sudo") {
        run_command(
            Command::new("sudo").arg("dpkg").arg("-i").arg(path),
            "install Debian package with sudo",
        )
    } else {
        Err(simple_error(
            "Installing the Linux package requires root, pkexec, or sudo",
        ))
    }
}

fn install_pkg(path: &Path) -> Result<()> {
    if cfg!(not(target_os = "macos")) {
        return Err(simple_error(
            "macOS packages can only be installed on macOS",
        ));
    }

    if is_root_user() {
        run_command(
            Command::new("installer")
                .arg("-pkg")
                .arg(path)
                .arg("-target")
                .arg("/"),
            "install macOS package",
        )
    } else if command_available("osascript") {
        let command = v_concat!("installer -pkg {} -target /", shell_single_quote(path));
        let script = v_concat!(
            "do shell script {} with administrator privileges",
            applescript_quote(&command)
        );
        run_command(
            Command::new("osascript").arg("-e").arg(script),
            "install macOS package with administrator privileges",
        )
    } else if command_available("sudo") {
        run_command(
            Command::new("sudo")
                .arg("installer")
                .arg("-pkg")
                .arg(path)
                .arg("-target")
                .arg("/"),
            "install macOS package with sudo",
        )
    } else {
        Err(simple_error(
            "Installing the macOS package requires administrator privileges",
        ))
    }
}

fn install_msi(path: &Path) -> Result<()> {
    if cfg!(not(target_os = "windows")) {
        return Err(simple_error(
            "MSI installers can only be installed on Windows",
        ));
    }

    let script = v_concat!(
        "$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', {}, '/qn', '/norestart') -Wait -PassThru -Verb RunAs; exit $p.ExitCode",
        powershell_quote(path)
    );
    powershell_status(&script)
}

fn install_exe(path: &Path) -> Result<()> {
    if cfg!(not(target_os = "windows")) {
        return Err(simple_error(
            "EXE installers can only be installed on Windows",
        ));
    }

    let script = v_concat!(
        "$p = Start-Process -FilePath {} -ArgumentList @('/quiet', '/norestart') -Wait -PassThru -Verb RunAs; exit $p.ExitCode",
        powershell_quote(path)
    );
    powershell_status(&script)
}

fn install_archive(archive: &Path, lower_name: &str) -> Result<()> {
    if cfg!(target_os = "windows") {
        return Err(simple_error(
            "Portable ZIP updates cannot replace a running Windows executable. Use the MSI or EXE release asset",
        ));
    }

    let parent = archive
        .parent()
        .ok_or_else(|| simple_error("update archive has no parent directory"))?;
    let extract_dir = parent.join("extract");
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("failed to create '{}'", extract_dir.display()))?;

    if lower_name.ends_with(".tar.gz") {
        run_command(
            Command::new("tar")
                .arg("-xzf")
                .arg(archive)
                .arg("-C")
                .arg(&extract_dir),
            "extract update archive",
        )?;
    } else {
        run_command(
            Command::new("unzip")
                .arg("-q")
                .arg(archive)
                .arg("-d")
                .arg(&extract_dir),
            "extract update zip",
        )?;
    }

    replace_installed_binary(&extract_dir)
}

fn replace_installed_binary(extract_dir: &Path) -> Result<()> {
    let current = std::env::current_exe().context("failed to locate the current executable")?;
    let name = current
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(APP_NAME)
        .to_owned();
    let source = match find_file_named(extract_dir, &name)? {
        Some(source) => source,
        None => find_file_named(extract_dir, APP_NAME)?
            .ok_or_else(|| simple_error(v_concat!("update archive did not contain {name}")))?,
    };

    replace_executable(&source, &current)
}

fn find_file_named(dir: &Path, file_name: &str) -> Result<Option<PathBuf>> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read '{}'", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, file_name)? {
                return Ok(Some(found));
            }
        } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

fn is_raw_binary_asset(asset_name: &str) -> bool {
    asset_name.starts_with(APP_NAME)
        && (asset_name.ends_with("_x86_64") || asset_name.ends_with("_arm64"))
}

fn install_binary_over_current(new_binary: &Path) -> Result<()> {
    let current = std::env::current_exe().context("failed to resolve current executable path")?;
    replace_executable(new_binary, &current)
}

fn replace_executable(source: &Path, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| simple_error("target executable has no parent directory"))?;
    let name = target
        .file_name()
        .ok_or_else(|| simple_error("target executable has no file name"))?
        .to_string_lossy();
    let staged = parent.join(v_concat!(".{name}.update-{}", std::process::id()));

    if fs::copy(source, &staged).is_ok() {
        set_executable(&staged)?;
        match fs::rename(&staged, target) {
            Ok(()) => return Ok(()),
            Err(_) => {
                let _ = fs::remove_file(&staged);
            }
        }
    }

    run_privileged(
        "install",
        vec![
            OsString::from("-m"),
            OsString::from("0755"),
            source.as_os_str().to_os_string(),
            target.as_os_str().to_os_string(),
        ],
    )
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
    if cfg!(windows) || is_root_user() {
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

pub(super) fn is_debian_like() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }

    if command_available("apt") || command_available("apt-get") || command_available("dpkg") {
        return true;
    }

    let Ok(os_release) = fs::read_to_string("/etc/os-release") else {
        return false;
    };
    let lower = os_release.to_ascii_lowercase();
    lower.contains("id=debian") || lower.contains("id=ubuntu") || lower.contains("id_like=debian")
}

pub(super) fn command_available(program: &str) -> bool {
    if cfg!(target_os = "windows") {
        Command::new("where.exe")
            .arg(program)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(v_concat!("command -v {program} >/dev/null 2>&1"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
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

fn is_root_user() -> bool {
    if cfg!(target_os = "windows") {
        return false;
    }

    Command::new("id")
        .arg("-u")
        .stdin(Stdio::null())
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "0"
        })
        .unwrap_or(false)
}

fn powershell_status(script: &str) -> Result<()> {
    let program = if command_available("powershell.exe") {
        "powershell.exe"
    } else {
        "pwsh"
    };
    let mut command = Command::new(program);
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    run_command(&mut command, "run Windows installer")
}

fn powershell_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    v_concat!("'{}'", value.replace('\'', "''"))
}

fn shell_single_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    v_concat!("'{}'", value.replace('\'', "'\\''"))
}

fn applescript_quote(value: &str) -> String {
    v_concat!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
