use std::cmp::Ordering;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Context, Result, simple_error};
use v_concat::v_concat;

mod install;
mod json;

use self::install::{command_available, install_downloaded_asset, run_command};
use self::json::{json_string_field, parse_assets};

const APP_NAME: &str = "v_fs_backup";
const GITHUB_REPO_URL: &str = "https://github.com/juanchoraf/v_fs_backup";
const GITHUB_API_LATEST: &str =
    "https://api.github.com/repos/juanchoraf/v_fs_backup/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubAsset {
    name: String,
    download_url: String,
}

pub(crate) fn check_update() -> Result<String> {
    let release = fetch_latest_release()?;
    let latest_version = normalize_version(&release.tag_name);
    let current_version = env!("CARGO_PKG_VERSION");
    let candidates = compatible_asset_names(latest_version);
    let selected = select_asset(&release.assets, &candidates);

    let status = match compare_versions(latest_version, current_version) {
        Ordering::Greater => v_concat!("Update available: {current_version} -> {latest_version}"),
        Ordering::Equal => v_concat!("{APP_NAME} is up to date at {current_version}"),
        Ordering::Less => {
            v_concat!(
                "Installed version {current_version} is newer than GitHub latest {latest_version}"
            )
        }
    };

    let asset_line = if let Some(asset) = selected {
        v_concat!("Compatible release asset: {}", asset.name)
    } else {
        v_concat!(
            "No compatible release asset found for {} {}",
            env::consts::OS,
            normalized_arch()
        )
    };

    Ok(v_concat!(
        "{status}\nRepository: {GITHUB_REPO_URL}\nLatest tag: {}\n{asset_line}",
        release.tag_name
    ))
}

pub(crate) fn install_update() -> Result<String> {
    let release = fetch_latest_release()?;
    let latest_version = normalize_version(&release.tag_name);
    let current_version = env!("CARGO_PKG_VERSION");

    match compare_versions(latest_version, current_version) {
        Ordering::Greater => {}
        Ordering::Equal => {
            return Ok(v_concat!(
                "{APP_NAME} is already up to date at {current_version}"
            ));
        }
        Ordering::Less => {
            return Ok(v_concat!(
                "Installed version {current_version} is newer than GitHub latest {latest_version}"
            ));
        }
    }

    let candidates = compatible_asset_names(latest_version);
    let asset = select_asset(&release.assets, &candidates).ok_or_else(|| {
        simple_error(v_concat!(
            "no compatible release asset found for {} {}; expected one of: {}",
            env::consts::OS,
            normalized_arch(),
            candidates.join(", ")
        ))
    })?;

    let temp_dir = prepare_temp_dir(latest_version)?;
    let artifact_path = temp_dir.join(&asset.name);
    download_file(&asset.download_url, &artifact_path)?;

    let checksum_message = verify_download_checksum(&release, asset, &artifact_path, &temp_dir)?;
    let install_message = install_downloaded_asset(&artifact_path, &asset.name)?;

    Ok(v_concat!(
        "Downloaded {}\n{}\n{}",
        asset.name,
        checksum_message,
        install_message
    ))
}

fn fetch_latest_release() -> Result<GitHubRelease> {
    let body = download_text(GITHUB_API_LATEST)?;
    let tag_name = json_string_field(&body, "tag_name")
        .ok_or_else(|| simple_error("GitHub latest release response did not include tag_name"))?;
    let assets = parse_assets(&body);

    Ok(GitHubRelease { tag_name, assets })
}

fn normalize_version(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = normalize_version(left);
    let right = normalize_version(right);
    let left_main = left.split_once('-').map_or(left, |(main, _)| main);
    let right_main = right.split_once('-').map_or(right, |(main, _)| main);
    let left_parts = left_main.split('.').collect::<Vec<_>>();
    let right_parts = right_main.split('.').collect::<Vec<_>>();
    let max_len = left_parts.len().max(right_parts.len());

    for index in 0..max_len {
        let left_part = left_parts
            .get(index)
            .and_then(|part| part.parse::<u64>().ok())
            .unwrap_or(0);
        let right_part = right_parts
            .get(index)
            .and_then(|part| part.parse::<u64>().ok())
            .unwrap_or(0);

        match left_part.cmp(&right_part) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    left.cmp(right)
}

fn compatible_asset_names(version: &str) -> Vec<String> {
    let arch = normalized_arch();
    let versioned_name = v_concat!("{APP_NAME}_v{version}");
    let mut names = Vec::new();

    match env::consts::OS {
        "windows" => {
            push_artifact(&mut names, &versioned_name, "windows", &arch, "exe");
            push_artifact(&mut names, &versioned_name, "windows", &arch, "msi");
            push_artifact(&mut names, &versioned_name, "windows", &arch, "zip");
        }
        "macos" => {
            push_artifact(&mut names, &versioned_name, "macos", &arch, "pkg");
            push_artifact(&mut names, &versioned_name, "macos", &arch, "");
            push_artifact(&mut names, &versioned_name, "macos", &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, "macos", &arch, "zip");
        }
        "linux" => {
            if is_debian_like() {
                push_artifact(&mut names, &versioned_name, "linux", &arch, "deb");
            }
            push_artifact(&mut names, &versioned_name, "linux", &arch, "");
            push_artifact(&mut names, &versioned_name, "linux", &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, "linux", &arch, "zip");
        }
        other_unix => {
            push_artifact(&mut names, &versioned_name, other_unix, &arch, "");
            push_artifact(&mut names, &versioned_name, other_unix, &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, other_unix, &arch, "zip");
            push_artifact(&mut names, &versioned_name, "unix", &arch, "");
            push_artifact(&mut names, &versioned_name, "unix", &arch, "tar.gz");
            push_artifact(&mut names, &versioned_name, "unix", &arch, "zip");
        }
    }

    names
}

fn push_artifact(names: &mut Vec<String>, versioned_name: &str, os: &str, arch: &str, ext: &str) {
    if ext.is_empty() {
        names.push(v_concat!("{versioned_name}_{os}_{arch}"));
    } else {
        names.push(v_concat!("{versioned_name}_{os}_{arch}.{ext}"));
    }
}

fn normalized_arch() -> String {
    match env::consts::ARCH {
        "aarch64" => "arm64".to_owned(),
        "x86_64" => "x86_64".to_owned(),
        other => other.to_owned(),
    }
}

fn is_debian_like() -> bool {
    if command_available("apt") || command_available("apt-get") || command_available("dpkg") {
        return true;
    }

    let Ok(os_release) = fs::read_to_string("/etc/os-release") else {
        return false;
    };
    let lower = os_release.to_ascii_lowercase();
    lower.contains("id=debian") || lower.contains("id=ubuntu") || lower.contains("id_like=debian")
}

fn select_asset<'a>(assets: &'a [GitHubAsset], candidates: &[String]) -> Option<&'a GitHubAsset> {
    candidates
        .iter()
        .find_map(|candidate| assets.iter().find(|asset| asset.name == *candidate))
}

fn checksum_name_for(asset_name: &str) -> String {
    let base = asset_name
        .strip_suffix(".tar.gz")
        .or_else(|| asset_name.strip_suffix(".zip"))
        .or_else(|| asset_name.strip_suffix(".deb"))
        .or_else(|| asset_name.strip_suffix(".pkg"))
        .or_else(|| asset_name.strip_suffix(".exe"))
        .or_else(|| asset_name.strip_suffix(".msi"))
        .unwrap_or(asset_name);

    v_concat!("{base}.sha256")
}

fn verify_download_checksum(
    release: &GitHubRelease,
    asset: &GitHubAsset,
    artifact_path: &Path,
    temp_dir: &Path,
) -> Result<String> {
    let checksum_name = checksum_name_for(&asset.name);
    let Some(checksum_asset) = release
        .assets
        .iter()
        .find(|release_asset| release_asset.name == checksum_name)
    else {
        return Ok(v_concat!(
            "Warning: no SHA-256 checksum asset found for {}",
            asset.name
        ));
    };

    let checksum_path = temp_dir.join(&checksum_asset.name);
    download_file(&checksum_asset.download_url, &checksum_path)?;
    let checksum_text = fs::read_to_string(&checksum_path)
        .with_context(|| format!("failed to read checksum file '{}'", checksum_path.display()))?;
    let expected = expected_checksum(&checksum_text, &asset.name).ok_or_else(|| {
        simple_error(v_concat!(
            "checksum file '{}' does not include {}",
            checksum_asset.name,
            asset.name
        ))
    })?;
    let actual = file_sha256(artifact_path)?;

    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(simple_error(v_concat!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            asset.name,
            expected,
            actual
        )));
    }

    Ok(v_concat!("Verified SHA-256: {actual}"))
}

fn expected_checksum(checksum_text: &str, asset_name: &str) -> Option<String> {
    checksum_text.lines().find_map(|line| {
        if !line.contains(asset_name) {
            return None;
        }

        let checksum = line.split_whitespace().next()?;
        if checksum.len() == 64 && checksum.chars().all(|ch| ch.is_ascii_hexdigit()) {
            Some(checksum.to_owned())
        } else {
            None
        }
    })
}

fn prepare_temp_dir(version: &str) -> Result<PathBuf> {
    let dir = env::temp_dir().join(v_concat!(
        "{APP_NAME}_update_{version}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create update temp directory '{}'", dir.display()))?;
    Ok(dir)
}

fn download_text(url: &str) -> Result<String> {
    #[cfg(windows)]
    {
        let script = "$ErrorActionPreference = 'Stop'; [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; (Invoke-WebRequest -UseBasicParsing -Uri $env:V_FS_BACKUP_UPDATE_URL -Headers @{Accept='application/vnd.github+json'; 'User-Agent'='v_fs_backup'}).Content";
        return powershell_output(script, &[("V_FS_BACKUP_UPDATE_URL", url)]);
    }

    #[cfg(not(windows))]
    {
        if let Ok(output) = Command::new("curl")
            .args([
                "-fsSL",
                "-H",
                "Accept: application/vnd.github+json",
                "-H",
                "User-Agent: v_fs_backup",
                url,
            ])
            .output()
        {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
        }

        if let Ok(output) = Command::new("fetch").args(["-qo", "-", url]).output() {
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
        }

        Err(simple_error(
            "failed to download release metadata; install curl or fetch and verify network access",
        ))
    }
}

fn download_file(url: &str, path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let path_str = path_to_string(path)?;
        let script = "$ErrorActionPreference = 'Stop'; [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -UseBasicParsing -Uri $env:V_FS_BACKUP_UPDATE_URL -OutFile $env:V_FS_BACKUP_UPDATE_OUT -Headers @{'User-Agent'='v_fs_backup'}";
        powershell_output(
            script,
            &[
                ("V_FS_BACKUP_UPDATE_URL", url),
                ("V_FS_BACKUP_UPDATE_OUT", &path_str),
            ],
        )?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        if run_command(
            Command::new("curl").arg("-fL").arg("-o").arg(path).arg(url),
            "download release asset",
        )
        .is_ok()
        {
            return Ok(());
        }

        run_command(
            Command::new("fetch").arg("-o").arg(path).arg(url),
            "download release asset",
        )
    }
}

fn file_sha256(path: &Path) -> Result<String> {
    #[cfg(windows)]
    {
        let path_str = path_to_string(path)?;
        let script = "$ErrorActionPreference = 'Stop'; (Get-FileHash -Algorithm SHA256 -Path $env:V_FS_BACKUP_HASH_PATH).Hash.ToLowerInvariant()";
        return powershell_output(script, &[("V_FS_BACKUP_HASH_PATH", &path_str)])
            .map(|text| text.trim().to_owned());
    }

    #[cfg(not(windows))]
    {
        if let Ok(output) = Command::new("sha256sum").arg(path).output() {
            if output.status.success() {
                if let Some(hash) = String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .next()
                {
                    return Ok(hash.to_owned());
                }
            }
        }

        if let Ok(output) = Command::new("shasum")
            .arg("-a")
            .arg("256")
            .arg(path)
            .output()
        {
            if output.status.success() {
                if let Some(hash) = String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .next()
                {
                    return Ok(hash.to_owned());
                }
            }
        }

        Err(simple_error(
            "failed to compute SHA-256; install sha256sum or shasum",
        ))
    }
}

#[cfg(windows)]
fn powershell_output(script: &str, envs: &[(&str, &str)]) -> Result<String> {
    let mut last_error = None;
    for shell in ["pwsh", "powershell"] {
        let mut command = Command::new(shell);
        command.args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);
        for (name, value) in envs {
            command.env(name, value);
        }

        match command.output() {
            Ok(output) if output.status.success() => {
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
            Ok(output) => {
                last_error = Some(String::from_utf8_lossy(&output.stderr).trim().to_owned())
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Err(simple_error(v_concat!(
        "PowerShell update command failed: {}",
        last_error.unwrap_or_else(|| "PowerShell was not found".to_owned())
    )))
}

#[cfg(windows)]
fn path_to_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| simple_error(v_concat!("path '{}' is not valid UTF-8", path.display())))
}

#[cfg(test)]
mod tests {
    use super::{checksum_name_for, compare_versions, expected_checksum};
    use std::cmp::Ordering;

    #[test]
    fn version_comparison_uses_numeric_parts() {
        assert_eq!(compare_versions("0.1.10", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_versions("v0.1.5", "0.1.5"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.4", "0.1.5"), Ordering::Less);
    }

    #[test]
    fn checksum_names_match_packaging_scripts() {
        assert_eq!(
            checksum_name_for("v_fs_backup_v1.2.3_linux_x86_64.tar.gz"),
            "v_fs_backup_v1.2.3_linux_x86_64.sha256"
        );
        assert_eq!(
            checksum_name_for("v_fs_backup_v1.2.3_windows_x86_64.exe"),
            "v_fs_backup_v1.2.3_windows_x86_64.sha256"
        );
        assert_eq!(
            checksum_name_for("v_fs_backup_v1.2.3_linux_x86_64"),
            "v_fs_backup_v1.2.3_linux_x86_64.sha256"
        );
    }

    #[test]
    fn expected_checksum_reads_matching_asset_line() {
        let checksum = expected_checksum(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  app.zip\n",
            "app.zip",
        );

        assert_eq!(
            checksum.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }
}
