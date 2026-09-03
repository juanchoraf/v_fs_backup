use std::env;

use crate::Result;

const APP_NAME: &str = "v_fs_backup";
#[cfg(windows)]
const APP_EXE_NAME: &str = "v_fs_backup.exe";
const APP_DISPLAY_NAME: &str = "v_fs_backup";
const APP_ID: &str = "com.thevelasquez.v_fs_backup";
const MIME_TYPE: &str = "application/x-v-fs-backup";
#[cfg(windows)]
const PUBLISHER: &str = "TheVelasquez.com";

mod shared;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(unix, not(target_os = "macos")))]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(all(unix, not(target_os = "macos")))]
use unix as platform;
#[cfg(windows)]
use windows as platform;

pub fn install_current_executable() -> Result<String> {
    platform::install()
}

pub fn uninstall_current_installation() -> Result<String> {
    platform::uninstall()
}

pub fn should_auto_install_current_executable() -> Result<bool> {
    if cfg!(debug_assertions) || env::var_os("V_FS_BACKUP_NO_AUTO_INSTALL").is_some() {
        return Ok(false);
    }

    let executable = shared::current_exe()?;
    if platform::is_installed_path(&executable) {
        return Ok(false);
    }

    Ok(executable
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_release_binary_name))
}

fn is_release_binary_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("v_fs_backup_v")
        && (lower.ends_with("_x86_64")
            || lower.ends_with("_arm64")
            || lower.ends_with("_x86_64.exe")
            || lower.ends_with("_arm64.exe"))
}
