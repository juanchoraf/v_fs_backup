use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{APP_DISPLAY_NAME, APP_EXE_NAME, APP_NAME, MIME_TYPE, PUBLISHER, shared};
use crate::{Context, Result, simple_error};

const ICON_BYTES: &[u8] = include_bytes!("../../assets/v_fs_backup_logo.ico");
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn install() -> Result<String> {
    let source = shared::current_exe()?;
    if !is_elevated()? {
        return elevate_or_error(&source, "--install");
    }

    let install_dir = install_dir();
    let installed_exe = install_dir.join(APP_EXE_NAME);
    shared::copy_binary(&source, &installed_exe)?;
    shared::write_bytes(&install_dir.join("v_fs_backup.ico"), ICON_BYTES)?;

    create_start_menu_shortcut(&installed_exe, &install_dir)?;
    register_app_path(&installed_exe, &install_dir)?;
    add_to_machine_path(&install_dir)?;
    register_file_association(&installed_exe)?;
    register_uninstall_entry(&installed_exe, &install_dir)?;

    Ok(format!(
        "Installed {APP_NAME} for all users at {}.\nRun `v_fs_backup` from a new terminal or open it from the Start menu.",
        installed_exe.display()
    ))
}

pub fn uninstall() -> Result<String> {
    if !is_elevated()? {
        let source = shared::current_exe()?;
        return elevate_or_error(&source, "--uninstall");
    }

    let install_dir = install_dir();
    let current = shared::current_exe()?;
    remove_from_machine_path(&install_dir)?;
    unregister_uninstall_entry()?;
    unregister_file_association()?;
    unregister_app_path()?;
    shared::remove_file_if_exists(&start_menu_shortcut())?;

    if shared::same_path(&current, &install_dir.join(APP_EXE_NAME)) {
        schedule_install_dir_removal(&install_dir)?;
        return Ok(format!(
            "Uninstalled {APP_NAME}. Final file cleanup will finish after this process exits."
        ));
    }

    shared::remove_dir_if_exists(&install_dir)?;

    Ok(format!(
        "Uninstalled {APP_NAME} from {}.",
        install_dir.display()
    ))
}

pub fn is_installed_path(path: &Path) -> bool {
    shared::same_path(path, &install_dir().join(APP_EXE_NAME))
}

fn elevate_or_error(source: &Path, flag: &str) -> Result<String> {
    let script = format!(
        "$p = Start-Process -FilePath {} -ArgumentList {} -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        ps_single_path(source),
        ps_single(flag)
    );
    let status =
        powershell_status(&script).context("failed to request administrator privileges")?;
    if status.success() {
        return Ok(format!("Finished elevated {flag} for {APP_NAME}."));
    }

    Err(simple_error(format!(
        "Installing {APP_NAME} for all users requires Administrator privileges."
    )))
}

fn is_elevated() -> Result<bool> {
    let output = Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)",
        ])
        .output()
        .context("failed to check Windows administrator status")?;

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .eq_ignore_ascii_case("true"))
}

fn install_dir() -> PathBuf {
    let base = env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    base.join(PUBLISHER).join(APP_NAME)
}

fn start_menu_shortcut() -> PathBuf {
    let base = env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(PUBLISHER)
        .join(format!("{APP_DISPLAY_NAME}.lnk"))
}

fn create_start_menu_shortcut(exe: &Path, install_dir: &Path) -> Result<()> {
    let shortcut = start_menu_shortcut();
    if let Some(parent) = shortcut.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let arguments = powershell_shortcut_arguments(exe);
    let script = format!(
        "$shortcut = {}\n\
$powerShell = Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe'\n\
$arguments = {}\n\
$workdir = {}\n\
$shell = New-Object -ComObject WScript.Shell\n\
$link = $shell.CreateShortcut($shortcut)\n\
$link.TargetPath = $powerShell\n\
$link.Arguments = $arguments\n\
$link.WorkingDirectory = $workdir\n\
$link.IconLocation = '{}'\n\
$link.Description = '{} by {}'\n\
$link.Save()",
        ps_single_path(&shortcut),
        ps_single(&arguments),
        ps_single_path(install_dir),
        format!("{},0", path_string(exe)).replace('\'', "''"),
        APP_DISPLAY_NAME.replace('\'', "''"),
        PUBLISHER.replace('\'', "''")
    );
    run_powershell(&script, "failed to create Windows Start menu shortcut")
}

fn powershell_shortcut_arguments(exe: &Path) -> String {
    format!(
        "-NoLogo -NoExit -ExecutionPolicy Bypass -Command \"$env:V_FS_BACKUP_INSIDE_POWERSHELL='1'; & '{}'\"",
        path_string(exe).replace('\'', "''")
    )
}

fn register_app_path(exe: &Path, install_dir: &Path) -> Result<()> {
    let key = format!(r"HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\{APP_EXE_NAME}");
    reg_add_default(&key, &path_string(exe))?;
    reg_add_value(&key, "Path", &path_string(install_dir))
}

fn unregister_app_path() -> Result<()> {
    let key = format!(r"HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\{APP_EXE_NAME}");
    reg_delete(&key)
}

fn register_file_association(exe: &Path) -> Result<()> {
    let app_key = format!(r"HKLM\Software\Classes\{APP_NAME}.archive");
    let icon = format!("{},0", quoted_path(exe));
    reg_add_default(
        r"HKLM\Software\Classes\.fsb",
        &format!("{APP_NAME}.archive"),
    )?;
    reg_add_value(r"HKLM\Software\Classes\.fsb", "Content Type", MIME_TYPE)?;
    reg_add_default(&app_key, &format!("{APP_DISPLAY_NAME} archive"))?;
    reg_add_default(&format!(r"{app_key}\DefaultIcon"), &icon)?;
    reg_add_default(&format!(r"{app_key}\shell\open\command"), &quoted_path(exe))
}

fn unregister_file_association() -> Result<()> {
    reg_delete(r"HKLM\Software\Classes\.fsb")?;
    reg_delete(&format!(r"HKLM\Software\Classes\{APP_NAME}.archive"))
}

fn register_uninstall_entry(exe: &Path, install_dir: &Path) -> Result<()> {
    let key = uninstall_entry_key();
    let uninstall_command = format!("{} --uninstall", quoted_path(exe));
    reg_add_default(&key, APP_DISPLAY_NAME)?;
    reg_add_value(&key, "DisplayName", APP_DISPLAY_NAME)?;
    reg_add_value(&key, "DisplayVersion", env!("CARGO_PKG_VERSION"))?;
    reg_add_value(&key, "Publisher", PUBLISHER)?;
    reg_add_value(&key, "InstallLocation", &path_string(install_dir))?;
    reg_add_value(&key, "DisplayIcon", &format!("{},0", quoted_path(exe)))?;
    reg_add_value(&key, "UninstallString", &uninstall_command)?;
    reg_add_value(&key, "QuietUninstallString", &uninstall_command)?;
    reg_add_value(&key, "NoModify", "1")?;
    reg_add_value(&key, "NoRepair", "1")
}

fn unregister_uninstall_entry() -> Result<()> {
    reg_delete(&uninstall_entry_key())
}

fn uninstall_entry_key() -> String {
    format!(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\{APP_NAME}")
}

fn schedule_install_dir_removal(install_dir: &Path) -> Result<()> {
    let script_path =
        env::temp_dir().join(format!("{APP_NAME}_uninstall_{}.ps1", std::process::id()));
    let script = r#"
param(
    [Parameter(Mandatory = $true)][string]$InstallDir,
    [Parameter(Mandatory = $true)][int]$PidToWait
)
$ErrorActionPreference = 'SilentlyContinue'
try {
    Wait-Process -Id $PidToWait -Timeout 120 -ErrorAction SilentlyContinue
}
catch {}
Start-Sleep -Milliseconds 300
Remove-Item -LiteralPath $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
"#;
    shared::write_text(&script_path, script)?;

    Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(&script_path)
        .arg("-InstallDir")
        .arg(install_dir)
        .arg("-PidToWait")
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to schedule final Windows uninstall cleanup")?;
    Ok(())
}

fn add_to_machine_path(install_dir: &Path) -> Result<()> {
    let script = format!(
        "$dir = {}\n\
$current = [Environment]::GetEnvironmentVariable('Path', 'Machine')\n\
$parts = @()\n\
if ($current) {{ $parts = $current -split ';' | Where-Object {{ $_ -and $_.Trim() }} }}\n\
$exists = $false\n\
foreach ($part in $parts) {{ if ($part.TrimEnd('\\') -ieq $dir.TrimEnd('\\')) {{ $exists = $true }} }}\n\
if (-not $exists) {{ [Environment]::SetEnvironmentVariable('Path', (($parts + $dir) -join ';'), 'Machine') }}",
        ps_single_path(install_dir)
    );
    run_powershell(&script, "failed to add v_fs_backup to the machine PATH")
}

fn remove_from_machine_path(install_dir: &Path) -> Result<()> {
    let script = format!(
        "$dir = {}\n\
$current = [Environment]::GetEnvironmentVariable('Path', 'Machine')\n\
if ($current) {{\n\
  $parts = $current -split ';' | Where-Object {{ $_ -and $_.Trim() -and ($_.TrimEnd('\\') -ine $dir.TrimEnd('\\')) }}\n\
  [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'Machine')\n\
}}",
        ps_single_path(install_dir)
    );
    run_powershell(
        &script,
        "failed to remove v_fs_backup from the machine PATH",
    )
}

fn reg_add_default(key: &str, value: &str) -> Result<()> {
    reg_status(
        ["add", key, "/ve", "/d", value, "/f"],
        "failed to write Windows registry value",
    )
}

fn reg_add_value(key: &str, name: &str, value: &str) -> Result<()> {
    reg_status(
        ["add", key, "/v", name, "/d", value, "/f"],
        "failed to write Windows registry value",
    )
}

fn reg_delete(key: &str) -> Result<()> {
    let status = Command::new("reg.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["delete", key, "/f"])
        .status()
        .with_context(|| format!("failed to run reg.exe for {key}"))?;
    if status.success() {
        return Ok(());
    }
    Ok(())
}

fn reg_status<const N: usize>(args: [&str; N], description: &str) -> Result<()> {
    let status = Command::new("reg.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .status()
        .context(description)?;
    if status.success() {
        Ok(())
    } else {
        Err(simple_error(format!(
            "{description}: reg.exe exited with {status}"
        )))
    }
}

fn run_powershell(script: &str, description: &str) -> Result<()> {
    let status = powershell_status(script).context(description)?;
    if status.success() {
        Ok(())
    } else {
        Err(simple_error(format!(
            "{description}: PowerShell exited with {status}"
        )))
    }
}

fn powershell_status(script: &str) -> std::io::Result<std::process::ExitStatus> {
    Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .status()
}

fn ps_single_path(path: &Path) -> String {
    ps_single(&path_string(path))
}

fn ps_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn quoted_path(path: &Path) -> String {
    format!("\"{}\"", path_string(path).replace('"', "\\\""))
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

trait CommandCreationFlags {
    fn creation_flags(&mut self, flags: u32) -> &mut Self;
}

impl CommandCreationFlags for Command {
    fn creation_flags(&mut self, flags: u32) -> &mut Self {
        use std::os::windows::process::CommandExt;

        CommandExt::creation_flags(self, flags)
    }
}
