use std::path::Path;
use std::process::Command;

use super::{APP_DISPLAY_NAME, APP_ID, APP_NAME, MIME_TYPE, shared};
use crate::{Context, Result, simple_error};

const INSTALL_BIN: &str = "/usr/local/bin/v_fs_backup";

pub fn install() -> Result<String> {
    let source = shared::current_exe()?;
    if !shared::is_root() {
        return elevate_or_error(&source, "--install");
    }

    shared::copy_binary(&source, Path::new(INSTALL_BIN))?;

    #[cfg(target_os = "linux")]
    install_linux_metadata()?;

    Ok(format!(
        "Installed {APP_NAME} for all users at {INSTALL_BIN}.\nRun `v_fs_backup` from a new terminal."
    ))
}

pub fn uninstall() -> Result<String> {
    if !shared::is_root() {
        let source = shared::current_exe()?;
        return elevate_or_error(&source, "--uninstall");
    }

    shared::remove_file_if_exists(Path::new(INSTALL_BIN))?;

    #[cfg(target_os = "linux")]
    uninstall_linux_metadata()?;

    Ok(format!("Uninstalled {APP_NAME} from {INSTALL_BIN}."))
}

pub fn is_installed_path(path: &Path) -> bool {
    shared::same_path(path, Path::new(INSTALL_BIN))
}

fn elevate_or_error(source: &Path, flag: &str) -> Result<String> {
    #[cfg(target_os = "linux")]
    {
        if graphical_session_available() && shared::command_available("pkexec") {
            let status = Command::new("pkexec")
                .arg(source)
                .arg(flag)
                .env("V_FS_BACKUP_NO_AUTO_INSTALL", "1")
                .status()
                .context("failed to start pkexec")?;
            if status.success() {
                return Ok(format!("Finished elevated {flag} for {APP_NAME}."));
            }
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

#[cfg(target_os = "linux")]
fn graphical_session_available() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(target_os = "linux")]
fn install_linux_metadata() -> Result<()> {
    for (size, bytes) in LINUX_ICONS {
        let icon_path = format!("/usr/share/icons/hicolor/{size}x{size}/apps/{APP_NAME}.png");
        shared::write_bytes(Path::new(&icon_path), bytes)?;
    }

    let desktop = format!(
        "[Desktop Entry]\n\
Type=Application\n\
Name={APP_DISPLAY_NAME}\n\
Comment=Fast compressed filesystem backups\n\
Exec={APP_NAME}\n\
Icon={APP_NAME}\n\
Terminal=true\n\
Categories=Utility;Archiving;\n\
Keywords=backup;archive;compression;filesystem;\n\
MimeType={MIME_TYPE};\n\
StartupWMClass={APP_NAME}\n"
    );
    let desktop_path = format!("/usr/share/applications/{APP_ID}.desktop");
    shared::write_text(Path::new(&desktop_path), &desktop)?;

    let mime = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n\
  <mime-type type=\"{MIME_TYPE}\">\n\
    <comment>{APP_DISPLAY_NAME} archive</comment>\n\
    <glob pattern=\"*.fsb\"/>\n\
    <icon name=\"{APP_NAME}\"/>\n\
  </mime-type>\n\
</mime-info>\n"
    );
    let mime_path = format!("/usr/share/mime/packages/{APP_NAME}.xml");
    shared::write_text(Path::new(&mime_path), &mime)?;

    shared::run_optional_command("update-mime-database", &["/usr/share/mime"]);
    shared::run_optional_command("update-desktop-database", &["/usr/share/applications"]);
    shared::run_optional_command(
        "gtk-update-icon-cache",
        &["-q", "-t", "-f", "/usr/share/icons/hicolor"],
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_linux_metadata() -> Result<()> {
    for (size, _) in LINUX_ICONS {
        let icon_path = format!("/usr/share/icons/hicolor/{size}x{size}/apps/{APP_NAME}.png");
        shared::remove_file_if_exists(Path::new(&icon_path))?;
    }

    let desktop_path = format!("/usr/share/applications/{APP_ID}.desktop");
    let mime_path = format!("/usr/share/mime/packages/{APP_NAME}.xml");
    shared::remove_file_if_exists(Path::new(&desktop_path))?;
    shared::remove_file_if_exists(Path::new(&mime_path))?;

    shared::run_optional_command("update-mime-database", &["/usr/share/mime"]);
    shared::run_optional_command("update-desktop-database", &["/usr/share/applications"]);
    shared::run_optional_command(
        "gtk-update-icon-cache",
        &["-q", "-t", "-f", "/usr/share/icons/hicolor"],
    );
    Ok(())
}

#[cfg(target_os = "linux")]
const ICON_16: &[u8; 583] = include_bytes!("../../assets/v_fs_backup_logo_16.png");
#[cfg(target_os = "linux")]
const ICON_24: &[u8; 1091] = include_bytes!("../../assets/v_fs_backup_logo_24.png");
#[cfg(target_os = "linux")]
const ICON_32: &[u8; 1859] = include_bytes!("../../assets/v_fs_backup_logo_32.png");
#[cfg(target_os = "linux")]
const ICON_48: &[u8; 3940] = include_bytes!("../../assets/v_fs_backup_logo_48.png");
#[cfg(target_os = "linux")]
const ICON_64: &[u8; 6906] = include_bytes!("../../assets/v_fs_backup_logo_64.png");
#[cfg(target_os = "linux")]
const ICON_128: &[u8; 26431] = include_bytes!("../../assets/v_fs_backup_logo_128.png");
#[cfg(target_os = "linux")]
const ICON_256: &[u8; 102612] = include_bytes!("../../assets/v_fs_backup_logo_256.png");
#[cfg(target_os = "linux")]
const ICON_512: &[u8; 397912] = include_bytes!("../../assets/v_fs_backup_logo_512.png");
#[cfg(target_os = "linux")]
const ICON_1024: &[u8; 1527202] = include_bytes!("../../assets/v_fs_backup_logo_1024.png");

#[cfg(target_os = "linux")]
const LINUX_ICONS: &[(u32, &[u8])] = &[
    (16, ICON_16),
    (24, ICON_24),
    (32, ICON_32),
    (48, ICON_48),
    (64, ICON_64),
    (128, ICON_128),
    (256, ICON_256),
    (512, ICON_512),
    (1024, ICON_1024),
];
