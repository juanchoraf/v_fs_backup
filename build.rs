use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_NAME: &str = "v_fs_backup";
const PUBLISHER: &str = "TheVelasquez.com";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/v_fs_backup_logo.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");

    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        if let Err(error) = embed_windows_resources() {
            if env::var("PROFILE").is_ok_and(|profile| profile == "release") {
                panic!("failed to embed Windows resources: {error}");
            }
            println!("cargo:warning=Windows resources were not embedded: {error}");
        }
    }
}

fn embed_windows_resources() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "CARGO_MANIFEST_DIR is not set")
        })?);
    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "OUT_DIR is not set"))?,
    );
    let icon_path = manifest_dir.join("assets").join("v_fs_backup_logo.ico");
    let rc_path = out_dir.join(format!("{APP_NAME}.rc"));

    write_rc_file(&rc_path, &icon_path)?;
    compile_resource(&rc_path, &out_dir)?;
    Ok(())
}

fn compile_resource(rc_path: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if target_env == "gnu" {
        let compiler = find_program("windres.exe")
            .or_else(|| find_program("windres"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "windres was not found"))?;
        let object_path = out_dir.join(format!("{APP_NAME}.resource.o"));
        let status = Command::new(&compiler)
            .arg("--input")
            .arg(rc_path)
            .arg("--output")
            .arg(&object_path)
            .arg("--output-format")
            .arg("coff")
            .status()?;

        if !status.success() {
            return Err(format!("{} exited with {status}", compiler.display()).into());
        }

        println!(
            "cargo:rustc-link-arg-bin={APP_NAME}={}",
            object_path.display()
        );
        return Ok(());
    }

    let compiler = find_program("rc.exe")
        .or_else(|| find_program("llvm-rc.exe"))
        .or_else(find_windows_sdk_rc)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "rc.exe or llvm-rc.exe was not found",
            )
        })?;
    let resource_path = out_dir.join(format!("{APP_NAME}.res"));
    let status = Command::new(&compiler)
        .arg("/nologo")
        .arg(format!("/fo{}", resource_path.display()))
        .arg(rc_path)
        .status()?;

    if !status.success() {
        return Err(format!("{} exited with {status}", compiler.display()).into());
    }

    println!(
        "cargo:rustc-link-arg-bin={APP_NAME}={}",
        resource_path.display()
    );
    Ok(())
}

fn write_rc_file(rc_path: &Path, icon_path: &Path) -> io::Result<()> {
    let package_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
    let file_version = version_tuple(&package_version);
    let icon = rc_escape(icon_path);
    let rc = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {file_version}
PRODUCTVERSION {file_version}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "{PUBLISHER}\0"
            VALUE "FileDescription", "v_fs_backup\0"
            VALUE "FileVersion", "{package_version}\0"
            VALUE "InternalName", "{APP_NAME}\0"
            VALUE "LegalCopyright", "Copyright (c) {PUBLISHER}\0"
            VALUE "OriginalFilename", "{APP_NAME}.exe\0"
            VALUE "ProductName", "v_fs_backup\0"
            VALUE "ProductVersion", "{package_version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );
    fs::write(rc_path, rc)
}

fn version_tuple(version: &str) -> String {
    let mut values = [0_u16; 4];
    for (index, part) in version
        .split(|ch| ch == '.' || ch == '-' || ch == '+')
        .take(4)
        .enumerate()
    {
        values[index] = part.parse().unwrap_or(0);
    }
    format!("{},{},{},{}", values[0], values[1], values[2], values[3])
}

fn rc_escape(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
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

fn find_windows_sdk_rc() -> Option<PathBuf> {
    let base = env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .map(|dir| dir.join("Windows Kits").join("10").join("bin"))?;
    let mut matches = fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("x64").join("rc.exe"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    matches.sort();
    matches.pop()
}
