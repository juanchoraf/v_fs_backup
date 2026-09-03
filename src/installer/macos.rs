use std::fs;
use std::path::Path;
use std::process::Command;

use super::{APP_DISPLAY_NAME, APP_ID, APP_NAME, MIME_TYPE, shared};
use crate::{Context, Result, simple_error};

const INSTALL_BIN: &str = "/usr/local/bin/v_fs_backup";
const APP_BUNDLE: &str = "/Applications/v_fs_backup.app";
const ICON_NAME: &str = "v_fs_backup_logo.icns";
const ICON_BYTES: &[u8] = include_bytes!("../../assets/v_fs_backup_logo.icns");

pub fn install() -> Result<String> {
    let source = shared::current_exe()?;
    if !shared::is_root() {
        return elevate_or_error(&source, "--install");
    }

    shared::copy_binary(&source, Path::new(INSTALL_BIN))?;
    install_app_bundle()?;
    register_launch_services();

    Ok(format!(
        "Installed {APP_NAME} for all users at {INSTALL_BIN} and {APP_BUNDLE}.\nRun `v_fs_backup` from a new terminal or open v_fs_backup from Applications."
    ))
}

pub fn uninstall() -> Result<String> {
    if !shared::is_root() {
        let source = shared::current_exe()?;
        return elevate_or_error(&source, "--uninstall");
    }

    shared::remove_file_if_exists(Path::new(INSTALL_BIN))?;
    shared::remove_dir_if_exists(Path::new(APP_BUNDLE))?;
    register_launch_services();

    Ok(format!(
        "Uninstalled {APP_NAME} from {INSTALL_BIN} and {APP_BUNDLE}."
    ))
}

pub fn is_installed_path(path: &Path) -> bool {
    shared::same_path(path, Path::new(INSTALL_BIN))
}

fn elevate_or_error(source: &Path, flag: &str) -> Result<String> {
    if shared::command_available("osascript") {
        let command = format!(
            "{} {flag}",
            shared::quote_sh_single(&source.to_string_lossy())
        );
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            command.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let status = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .env("V_FS_BACKUP_NO_AUTO_INSTALL", "1")
            .status()
            .context("failed to request administrator privileges")?;
        if status.success() {
            return Ok(format!("Finished elevated {flag} for {APP_NAME}."));
        }
    }

    if shared::command_available("sudo") {
        let status = Command::new("sudo")
            .arg(source)
            .arg(flag)
            .env("V_FS_BACKUP_NO_AUTO_INSTALL", "1")
            .status()
            .context("failed to start sudo")?;
        if status.success() {
            return Ok(format!("Finished elevated {flag} for {APP_NAME}."));
        }
    }

    Err(simple_error(format!(
        "Installing {APP_NAME} for all users requires administrator privileges.\nRun: sudo {} {flag}",
        shared::quote_sh_single(&source.to_string_lossy())
    )))
}

fn install_app_bundle() -> Result<()> {
    let bundle = Path::new(APP_BUNDLE);
    shared::remove_dir_if_exists(bundle)?;

    let macos_dir = bundle.join("Contents").join("MacOS");
    let resources_dir = bundle.join("Contents").join("Resources");
    fs::create_dir_all(&macos_dir)
        .with_context(|| format!("failed to create {}", macos_dir.display()))?;
    fs::create_dir_all(&resources_dir)
        .with_context(|| format!("failed to create {}", resources_dir.display()))?;

    let launcher = macos_dir.join(APP_NAME);
    let plist = bundle.join("Contents").join("Info.plist");
    let icon = resources_dir.join(ICON_NAME);

    shared::write_text(&launcher, MACOS_LAUNCHER)?;
    set_mode(&launcher, 0o755)?;
    shared::write_text(&plist, &info_plist())?;
    shared::write_bytes(&icon, ICON_BYTES)?;
    Ok(())
}

fn info_plist() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
  <key>CFBundleDevelopmentRegion</key>\n\
  <string>en</string>\n\
  <key>CFBundleDocumentTypes</key>\n\
  <array>\n\
    <dict>\n\
      <key>CFBundleTypeExtensions</key>\n\
      <array><string>fsb</string></array>\n\
      <key>CFBundleTypeIconFile</key>\n\
      <string>v_fs_backup_logo</string>\n\
      <key>CFBundleTypeName</key>\n\
      <string>{APP_DISPLAY_NAME} archive</string>\n\
      <key>CFBundleTypeRole</key>\n\
      <string>Viewer</string>\n\
      <key>LSHandlerRank</key>\n\
      <string>Owner</string>\n\
      <key>LSItemContentTypes</key>\n\
      <array><string>com.thevelasquez.v-fs-backup.archive</string></array>\n\
    </dict>\n\
  </array>\n\
  <key>CFBundleExecutable</key>\n\
  <string>{APP_NAME}</string>\n\
  <key>CFBundleIconFile</key>\n\
  <string>v_fs_backup_logo</string>\n\
  <key>CFBundleIdentifier</key>\n\
  <string>{APP_ID}</string>\n\
  <key>CFBundleName</key>\n\
  <string>{APP_DISPLAY_NAME}</string>\n\
  <key>CFBundlePackageType</key>\n\
  <string>APPL</string>\n\
  <key>CFBundleShortVersionString</key>\n\
  <string>{version}</string>\n\
  <key>CFBundleVersion</key>\n\
  <string>{version}</string>\n\
  <key>LSMinimumSystemVersion</key>\n\
  <string>10.13</string>\n\
  <key>UTExportedTypeDeclarations</key>\n\
  <array>\n\
    <dict>\n\
      <key>UTTypeConformsTo</key>\n\
      <array><string>public.data</string></array>\n\
      <key>UTTypeDescription</key>\n\
      <string>{APP_DISPLAY_NAME} archive</string>\n\
      <key>UTTypeIdentifier</key>\n\
      <string>com.thevelasquez.v-fs-backup.archive</string>\n\
      <key>UTTypeTagSpecification</key>\n\
      <dict>\n\
        <key>public.filename-extension</key>\n\
        <array><string>fsb</string></array>\n\
        <key>public.mime-type</key>\n\
        <string>{MIME_TYPE}</string>\n\
      </dict>\n\
    </dict>\n\
  </array>\n\
</dict>\n\
</plist>\n"
    )
}

fn register_launch_services() {
    let tool = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
    if Path::new(tool).is_file() {
        let _ = Command::new(tool).args(["-f", APP_BUNDLE]).status();
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to read permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

const MACOS_LAUNCHER: &str = r#"#!/bin/sh
/usr/bin/osascript <<'APPLESCRIPT'
tell application "Terminal"
    activate
    do script "/usr/local/bin/v_fs_backup"
end tell
APPLESCRIPT
"#;
