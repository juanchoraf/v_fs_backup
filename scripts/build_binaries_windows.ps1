#requires -version 5.1
[CmdletBinding()]
param(
    [switch]$Locked,
    [switch]$NoUpdate
)

$ErrorActionPreference = "Stop"
$AppName = "v_fs_backup"
$VersionsDir = "versions"

function v_concat {
    param([AllowNull()][string]$Value)
    return "`n$Value`n`n"
}

function Write-SpacedHost {
    param([AllowNull()][string]$Value)
    [Console]::Out.Write((v_concat $Value))
}

function Set-BuildPrivacy {
    $privacyDefaults = [ordered]@{
        "DOTNET_CLI_TELEMETRY_OPTOUT" = "1"
        "DOTNET_NOLOGO" = "true"
        "POWERSHELL_TELEMETRY_OPTOUT" = "1"
        "POWERSHELL_UPDATECHECK" = "Off"
        "POWERSHELL_DIAGNOSTICS_OPTOUT" = "1"
        "VSCMD_SKIP_SENDTELEMETRY" = "1"
        "RUSTUP_NO_UPDATE_CHECK" = "1"
        "DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE" = "1"
        "DOTNET_SKIP_FIRST_TIME_EXPERIENCE" = "1"
    }

    foreach ($name in $privacyDefaults.Keys) {
        [Environment]::SetEnvironmentVariable($name, $privacyDefaults[$name], "Process")
        Set-Item -Path "Env:$name" -Value $privacyDefaults[$name]
    }
}

function Get-PackageVersion {
    $cargoToml = Join-Path $RepoDir "Cargo.toml"
    foreach ($line in (Get-Content $cargoToml)) {
        if ($line -match '^\s*version\s*=\s*"([^"]+)"') {
            return $Matches[1]
        }
    }

    throw "Unable to read package version from $cargoToml"
}

function Get-PlatformArch {
    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($arch -eq "AMD64") {
        return "x86_64"
    }
    if ($arch -eq "ARM64") {
        return "arm64"
    }

    throw "Unsupported Windows architecture: $arch. Only 64-bit builds are supported."
}

function Invoke-CargoBuild {
    if (-not $NoUpdate -and -not $Locked) {
        cargo update
        if ($LASTEXITCODE -ne 0) {
            throw "cargo update failed with exit code $LASTEXITCODE"
        }
    }

    $args = @("build", "--release")
    if ($Locked) {
        $args += "--locked"
    }

    cargo @args
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
}

function Write-Checksum {
    param(
        [string]$Artifact,
        [string]$OutDir
    )

    $outPath = Join-Path $OutDir $Artifact
    $hash = (Get-FileHash -Algorithm SHA256 -Path $outPath).Hash.ToLowerInvariant()
    Set-Content -Encoding ASCII -Path "$outPath.sha256" -Value "$hash  $Artifact"
}

Set-BuildPrivacy

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoDir = Resolve-Path (Join-Path $ScriptDir "..")
Set-Location $RepoDir

$version = Get-PackageVersion
$arch = Get-PlatformArch
$versionedName = "${AppName}_v$version"
$outDir = Join-Path $VersionsDir $versionedName
$binary = Join-Path "target\release" "$AppName.exe"
$artifact = "${versionedName}_windows_$arch.exe"
$outPath = Join-Path $outDir $artifact

Invoke-CargoBuild

if (-not (Test-Path $binary)) {
    throw "release binary not found at $binary"
}

New-Item -ItemType Directory -Force -Path $outDir | Out-Null
Remove-Item $outPath -Force -ErrorAction SilentlyContinue
Remove-Item "$outPath.sha256" -Force -ErrorAction SilentlyContinue
Copy-Item $binary $outPath -Force
Write-Checksum -Artifact $artifact -OutDir $outDir

Write-SpacedHost "Windows binary created: $outPath"
