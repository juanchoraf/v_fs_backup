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

    if is_raw_binary_asset(asset_name) {
        return install_binary_over_current(path);
    }

    Err(simple_error(v_concat!(
        "downloaded {asset_name}, but this platform does not know how to install that asset type"
    )))
}

fn is_raw_binary_asset(asset_name: &str) -> bool {
    asset_name.starts_with(APP_NAME)
        && (asset_name.ends_with("_x86_64") || asset_name.ends_with("_arm64"))
}

fn install_windows_asset(path: &Path, asset_name: &str) -> Result<String> {
    if asset_name.ends_with(".exe") && asset_name.starts_with(APP_NAME) {
        return stage_windows_binary_update(path);
    }

    Err(simple_error(v_concat!(
        "downloaded {asset_name}, but Windows updates require the .exe binary asset"
    )))
}

fn stage_windows_binary_update(new_binary: &Path) -> Result<String> {
    let current = env::current_exe().context("failed to resolve current executable path")?;
    let temp_dir = prepare_temp_dir("windows-update")?;
    let staged = temp_dir.join(v_concat!("{APP_NAME}_update_{}.exe", std::process::id()));
    let script_path = temp_dir.join(v_concat!("{APP_NAME}_update_{}.ps1", std::process::id()));

    fs::copy(new_binary, &staged).with_context(|| {
        format!(
            "failed to stage Windows update binary from '{}' to '{}'",
            new_binary.display(),
            staged.display()
        )
    })?;

    let script = r#"
param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Target,
    [Parameter(Mandatory = $true)][int]$PidToWait
)
$ErrorActionPreference = 'Stop'
try {
    Wait-Process -Id $PidToWait -Timeout 120 -ErrorAction SilentlyContinue
}
catch {}
Start-Sleep -Milliseconds 300
Copy-Item -LiteralPath $Source -Destination $Target -Force
Remove-Item -LiteralPath $Source -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
"#;
    fs::write(&script_path, script).with_context(|| {
        format!(
            "failed to write Windows update script '{}'",
            script_path.display()
        )
    })?;

    let needs_elevation = !target_parent_is_writable(&current);
    if !spawn_windows_update_script(&script_path, &staged, &current, needs_elevation)?
        && !needs_elevation
    {
        spawn_windows_update_script(&script_path, &staged, &current, true)?;
    }

    Ok(
        "Staged the Windows binary update. Close and open v_fs_backup again to apply it."
            .to_owned(),
    )
}

fn target_parent_is_writable(target: &Path) -> bool {
    let Some(parent) = target.parent() else {
        return false;
    };
    let probe = parent.join(v_concat!(".{APP_NAME}_write_test_{}", std::process::id()));
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
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

#[cfg(windows)]
fn spawn_windows_update_script(
    script: &Path,
    source: &Path,
    target: &Path,
    elevated: bool,
) -> Result<bool> {
    let mut args = vec![
        "-NoLogo".to_owned(),
        "-NoProfile".to_owned(),
        "-ExecutionPolicy".to_owned(),
        "Bypass".to_owned(),
        "-WindowStyle".to_owned(),
        "Hidden".to_owned(),
        "-File".to_owned(),
        script.display().to_string(),
        "-Source".to_owned(),
        source.display().to_string(),
        "-Target".to_owned(),
        target.display().to_string(),
        "-PidToWait".to_owned(),
        std::process::id().to_string(),
    ];

    if elevated {
        let argument_list = args
            .drain(..)
            .map(|arg| format!("'{}'", arg.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        let script = format!(
            "Start-Process -FilePath 'powershell.exe' -ArgumentList @({argument_list}) -Verb RunAs"
        );
        run_command(
            Command::new("powershell.exe")
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                ])
                .arg(script)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
            "start elevated Windows binary updater",
        )?;
        return Ok(true);
    }

    Ok(Command::new("powershell.exe")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok())
}

#[cfg(not(windows))]
fn spawn_windows_update_script(
    _script: &Path,
    _source: &Path,
    _target: &Path,
    _elevated: bool,
) -> Result<bool> {
    Ok(false)
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
