#requires -version 5.1
[CmdletBinding()]
param(
    [switch]$Locked,
    [switch]$AllowUpdates,
    [switch]$AllowDownloads,
    [switch]$NoDownloads,
    [switch]$AcceptWixEula,
    [switch]$NoWixEula,
    [switch]$NoUpdate,
    [switch]$NoBuildTools,
    [switch]$NoWix,
    [switch]$SkipCodeSigning,
    [switch]$NoGenerateCodeSigningCertificate,
    [switch]$TrustGeneratedCodeSigningCertificateForAllUsers,
    [string]$CodeSigningCertPath = $env:V_FS_BACKUP_CODESIGN_PFX,
    [string]$CodeSigningCertPassword = $env:V_FS_BACKUP_CODESIGN_PASSWORD,
    [string]$CodeSigningCertThumbprint = $env:V_FS_BACKUP_CODESIGN_THUMBPRINT,
    [string]$GeneratedCodeSigningCertPassword = $env:V_FS_BACKUP_LOCAL_CODESIGN_PASSWORD,
    [string]$CodeSigningTimestampUrl = $(if ($env:V_FS_BACKUP_CODESIGN_TIMESTAMP_URL) { $env:V_FS_BACKUP_CODESIGN_TIMESTAMP_URL } else { "http://timestamp.digicert.com" })
)

$ErrorActionPreference = "Stop"
$AppName = "v_fs_backup"
$Publisher = "TheVelasquez.com"
$ExpectedSignaturePublisher = "TheVelasquez.com"
$RustTarget = "x86_64-pc-windows-msvc"
$WixBootstrapperExtension = "WixToolset.BootstrapperApplications.wixext"
$NeedsReboot = $false
$UsingGeneratedCodeSigningCertificate = $false
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function v_concat {
    param([AllowNull()][string]$Value)

    return "`n$Value`n`n"
}

function Write-Host {
    [CmdletBinding()]
    param(
        [Parameter(Position = 0)]
        [AllowNull()]
        [object]$Object,
        [ConsoleColor]$ForegroundColor
    )

    $message = [string]$Object
    if ($PSBoundParameters.ContainsKey("ForegroundColor")) {
        $previous = [Console]::ForegroundColor
        try {
            [Console]::ForegroundColor = $ForegroundColor
            [Console]::Out.Write((v_concat $message))
        }
        finally {
            [Console]::ForegroundColor = $previous
        }
        return
    }

    [Console]::Out.Write((v_concat $message))
}

function Write-Warning {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true, Position = 0)]
        [object]$Message
    )

    Microsoft.PowerShell.Utility\Write-Warning -Message (v_concat ([string]$Message))
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

Set-BuildPrivacy
if ($AllowUpdates -and $NoUpdate) {
    throw "Use either -AllowUpdates or -NoUpdate, not both."
}
if (-not $AllowUpdates) {
    $NoUpdate = $true
}
if ($AllowDownloads -and $NoDownloads) {
    throw "Use either -AllowDownloads or -NoDownloads, not both."
}
if (-not $NoDownloads) {
    $AllowDownloads = $true
}
if ($AcceptWixEula -and $NoWixEula) {
    throw "Use either -AcceptWixEula or -NoWixEula, not both."
}
if (-not $NoWixEula) {
    $AcceptWixEula = $true
}

Write-Host ""

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-CheckedProcess {
    param(
        [string]$FilePath,
        [string[]]$ArgumentList,
        [string]$Description
    )

    Write-Host $Description
    $quotedArguments = @($ArgumentList | ForEach-Object {
        if ($_ -match '[\s"]') {
            '"' + ($_ -replace '"', '\"') + '"'
        }
        else {
            $_
        }
    })
    $process = Start-Process -FilePath $FilePath -ArgumentList ($quotedArguments -join ' ') -Wait -PassThru
    if ($process.ExitCode -eq 3010) {
        $script:NeedsReboot = $true
        Write-Warning "$Description completed, but Windows reports a reboot is required."
        return
    }
    if ($process.ExitCode -ne 0) {
        throw "$Description failed with exit code $($process.ExitCode)"
    }
}

function Get-GeneratedCodeSigningCertificatePaths {
    $certsDir = Join-Path $RepoDir "certs"
    $safePublisher = $ExpectedSignaturePublisher -replace '[^A-Za-z0-9._-]', '_'
    $baseName = "$AppName-$safePublisher-local-codesign"

    return [pscustomobject]@{
        Directory = $certsDir
        Cer = Join-Path $certsDir "$baseName.cer"
        Pfx = Join-Path $certsDir "$baseName.pfx"
        Password = Join-Path $certsDir "$baseName.password.txt"
        Thumbprint = Join-Path $certsDir "$baseName.thumbprint.txt"
    }
}

function Get-GeneratedCodeSigningCertificateFriendlyName {
    return "$AppName local code signing ($ExpectedSignaturePublisher)"
}

function Test-VFsBackupCodeSigningCertificate {
    param([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)

    if ($null -eq $Certificate -or -not $Certificate.HasPrivateKey) {
        return $false
    }
    $now = Get-Date
    if ($Certificate.NotBefore -gt $now -or $Certificate.NotAfter -le $now) {
        return $false
    }
    $simpleName = $Certificate.GetNameInfo(
        [System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,
        $false
    )
    if ([string]::IsNullOrWhiteSpace($simpleName) -or -not $simpleName.Equals($ExpectedSignaturePublisher, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $false
    }

    $ekuExtension = $Certificate.Extensions |
        Where-Object { $_.Oid.Value -eq "2.5.29.37" } |
        Select-Object -First 1
    if ($ekuExtension) {
        $codeSigningOid = "1.3.6.1.5.5.7.3.3"
        $enhancedKeyUsage = [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]$ekuExtension
        foreach ($oid in $enhancedKeyUsage.EnhancedKeyUsages) {
            if ($oid.Value -eq $codeSigningOid) {
                return $true
            }
        }

        return $false
    }

    return $true
}

function New-GeneratedCertificatePassword {
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    }
    finally {
        $rng.Dispose()
    }

    return [Convert]::ToBase64String($bytes)
}

function Export-GeneratedCodeSigningCertificateFiles {
    param([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)

    $paths = Get-GeneratedCodeSigningCertificatePaths
    New-Item -ItemType Directory -Force -Path $paths.Directory | Out-Null
    Export-Certificate -Cert $Certificate -FilePath $paths.Cer -Force | Out-Null
    Set-Content -Encoding ASCII -Path $paths.Thumbprint -Value $Certificate.Thumbprint

    $password = $GeneratedCodeSigningCertPassword
    if ([string]::IsNullOrWhiteSpace($password)) {
        if (Test-Path $paths.Password) {
            $password = (Get-Content -Path $paths.Password -Raw).Trim()
        }
        else {
            $password = New-GeneratedCertificatePassword
            Set-Content -Encoding ASCII -Path $paths.Password -Value $password
        }
    }

    $securePassword = ConvertTo-SecureString -String $password -AsPlainText -Force
    Export-PfxCertificate -Cert $Certificate -FilePath $paths.Pfx -Password $securePassword -Force | Out-Null

    Write-Host "local code signing certificate files are under $($paths.Directory)"
}

function Import-GeneratedCodeSigningCertificateTrust {
    param([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)

    $paths = Get-GeneratedCodeSigningCertificatePaths
    if (-not (Test-Path $paths.Cer)) {
        Export-Certificate -Cert $Certificate -FilePath $paths.Cer -Force | Out-Null
    }

    foreach ($store in @("Cert:\CurrentUser\Root", "Cert:\CurrentUser\TrustedPublisher")) {
        Import-Certificate -FilePath $paths.Cer -CertStoreLocation $store | Out-Null
    }

    if ($TrustGeneratedCodeSigningCertificateForAllUsers) {
        if (-not (Test-Administrator)) {
            throw "Trusting the generated code signing certificate for all users requires Administrator PowerShell."
        }

        foreach ($store in @("Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher")) {
            Import-Certificate -FilePath $paths.Cer -CertStoreLocation $store | Out-Null
        }
    }
}

function Find-GeneratedCodeSigningCertificate {
    $friendlyName = Get-GeneratedCodeSigningCertificateFriendlyName
    $certificates = Get-ChildItem -Path "Cert:\CurrentUser\My" -ErrorAction SilentlyContinue |
        Where-Object { $_.FriendlyName -eq $friendlyName -and (Test-VFsBackupCodeSigningCertificate -Certificate $_) } |
        Sort-Object NotAfter -Descending

    return $certificates | Select-Object -First 1
}

function Import-GeneratedCodeSigningCertificateFromPfx {
    $paths = Get-GeneratedCodeSigningCertificatePaths
    if (-not (Test-Path $paths.Pfx) -or -not (Test-Path $paths.Password)) {
        return $null
    }

    $password = (Get-Content -Path $paths.Password -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($password)) {
        return $null
    }

    $securePassword = ConvertTo-SecureString -String $password -AsPlainText -Force
    $certificate = Import-PfxCertificate -FilePath $paths.Pfx -CertStoreLocation "Cert:\CurrentUser\My" -Password $securePassword
    if (Test-VFsBackupCodeSigningCertificate -Certificate $certificate) {
        return $certificate
    }

    return $null
}

function Get-OrCreateGeneratedCodeSigningCertificate {
    if ($NoGenerateCodeSigningCertificate) {
        throw "A trusted code signing certificate is required so Windows UAC shows Publisher: $ExpectedSignaturePublisher. Set V_FS_BACKUP_CODESIGN_PFX and V_FS_BACKUP_CODESIGN_PASSWORD, set V_FS_BACKUP_CODESIGN_THUMBPRINT, or remove -NoGenerateCodeSigningCertificate to create a local self-signed test certificate."
    }

    $certificate = Find-GeneratedCodeSigningCertificate
    if (-not $certificate) {
        $certificate = Import-GeneratedCodeSigningCertificateFromPfx
    }

    if (-not $certificate) {
        Write-Warning "No public CA-issued code signing certificate was configured. Creating a local self-signed test certificate for $ExpectedSignaturePublisher."
        $certificate = New-SelfSignedCertificate `
            -Type CodeSigningCert `
            -Subject "CN=$ExpectedSignaturePublisher" `
            -FriendlyName (Get-GeneratedCodeSigningCertificateFriendlyName) `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -KeyAlgorithm RSA `
            -KeyLength 3072 `
            -HashAlgorithm SHA256 `
            -KeyExportPolicy Exportable `
            -NotAfter (Get-Date).AddYears(3)
    }

    if (-not (Test-VFsBackupCodeSigningCertificate -Certificate $certificate)) {
        throw "Generated code signing certificate is not valid for $ExpectedSignaturePublisher."
    }

    Export-GeneratedCodeSigningCertificateFiles -Certificate $certificate
    Import-GeneratedCodeSigningCertificateTrust -Certificate $certificate
    $script:UsingGeneratedCodeSigningCertificate = $true
    Write-Warning "The generated certificate is trusted only on machines where its .cer is explicitly installed. Use a CA-issued or Microsoft Trusted Signing certificate for public releases."
    return $certificate
}

function Resolve-CodeSigningCertificate {
    if ($SkipCodeSigning) {
        Write-Warning "Building unsigned Windows artifacts. UAC will show Publisher: Unknown."
        return $null
    }

    if (-not [string]::IsNullOrWhiteSpace($CodeSigningCertThumbprint)) {
        $normalizedThumbprint = ($CodeSigningCertThumbprint -replace '\s', '').ToUpperInvariant()
        $certificateStores = @("Cert:\CurrentUser\My", "Cert:\LocalMachine\My")

        foreach ($store in $certificateStores) {
            if (-not (Test-Path $store)) {
                continue
            }

            $certificate = Get-ChildItem -Path $store -ErrorAction SilentlyContinue |
                Where-Object { ($_.Thumbprint -replace '\s', '').ToUpperInvariant() -eq $normalizedThumbprint } |
                Select-Object -First 1
            if ($certificate) {
                if (-not $certificate.HasPrivateKey) {
                    throw "Code signing certificate $CodeSigningCertThumbprint was found, but it does not include a private key."
                }
                return $certificate
            }
        }

        throw "Unable to find code signing certificate thumbprint $CodeSigningCertThumbprint in CurrentUser\My or LocalMachine\My."
    }

    if (-not [string]::IsNullOrWhiteSpace($CodeSigningCertPath)) {
        $resolvedPath = (Resolve-Path $CodeSigningCertPath).ProviderPath
        if ([string]::IsNullOrEmpty($CodeSigningCertPassword)) {
            $certificate = Get-PfxCertificate -FilePath $resolvedPath
        }
        else {
            $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable -bor [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::PersistKeySet -bor [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::UserKeySet
            $certificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new(
                $resolvedPath,
                $CodeSigningCertPassword,
                $flags
            )
        }

        if (-not $certificate.HasPrivateKey) {
            throw "Code signing certificate at $resolvedPath does not include a private key."
        }
        return $certificate
    }

    return Get-OrCreateGeneratedCodeSigningCertificate
}

function Assert-CodeSigningPublisher {
    param([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)

    $simpleName = $Certificate.GetNameInfo(
        [System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,
        $false
    )
    if ([string]::IsNullOrWhiteSpace($simpleName) -or -not $simpleName.Equals($ExpectedSignaturePublisher, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Code signing certificate issued-to name must be '$ExpectedSignaturePublisher' so UAC shows the expected publisher. Found: '$simpleName' ($($Certificate.Subject))"
    }
}

function Invoke-CodeSignFile {
    param(
        [AllowNull()][System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
        [string]$FilePath,
        [string]$Description
    )

    if ($null -eq $Certificate) {
        return
    }

    Assert-CodeSigningPublisher -Certificate $Certificate
    $signArgs = @{
        FilePath = $FilePath
        Certificate = $Certificate
        HashAlgorithm = "SHA256"
    }
    if (-not [string]::IsNullOrWhiteSpace($CodeSigningTimestampUrl)) {
        $signArgs["TimestampServer"] = $CodeSigningTimestampUrl
    }

    $signature = Set-AuthenticodeSignature @signArgs
    if ($signature.Status -ne "Valid") {
        $statusMessage = if ($signature.StatusMessage) { $signature.StatusMessage } else { "no status message returned" }
        throw "Code signing failed for $Description at $FilePath. Status: $($signature.Status). $statusMessage"
    }

    $verified = Get-AuthenticodeSignature -FilePath $FilePath
    if ($verified.Status -ne "Valid") {
        $statusMessage = if ($verified.StatusMessage) { $verified.StatusMessage } else { "no status message returned" }
        throw "Signed $Description, but verification failed at $FilePath. Status: $($verified.Status). $statusMessage"
    }

    Write-Host "signed $Description as $ExpectedSignaturePublisher"
}

function Invoke-CodeSignBundle {
    param(
        [AllowNull()][System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
        [string]$BundlePath,
        [string]$Description
    )

    if ($null -eq $Certificate) {
        return
    }

    $signDir = Join-Path $env:TEMP "$AppName-burn-sign-$([guid]::NewGuid().ToString('N'))"
    $enginePath = Join-Path $signDir "burn-engine.exe"
    $reattachedBundlePath = Join-Path $signDir "reattached-bundle.exe"

    New-Item -ItemType Directory -Force -Path $signDir | Out-Null
    try {
        & wix burn detach $BundlePath -engine $enginePath
        if ($LASTEXITCODE -ne 0) {
            throw "wix burn detach failed with exit code $LASTEXITCODE"
        }

        Invoke-CodeSignFile `
            -Certificate $Certificate `
            -FilePath $enginePath `
            -Description "$Description Burn engine"

        & wix burn reattach $BundlePath -engine $enginePath -o $reattachedBundlePath
        if ($LASTEXITCODE -ne 0) {
            throw "wix burn reattach failed with exit code $LASTEXITCODE"
        }

        Invoke-CodeSignFile `
            -Certificate $Certificate `
            -FilePath $reattachedBundlePath `
            -Description $Description

        Copy-Item $reattachedBundlePath $BundlePath -Force
    }
    finally {
        Remove-Item $signDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-ProgramFilesX86 {
    if (${env:ProgramFiles(x86)}) {
        return ${env:ProgramFiles(x86)}
    }

    return $env:ProgramFiles
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

function Get-VsWherePath {
    $vsWhere = Join-Path (Get-ProgramFilesX86) "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vsWhere) {
        return $vsWhere
    }

    $command = Get-Command vswhere.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    return $null
}

function Get-VisualStudioInstallPath {
    param([switch]$BuildToolsOnly)

    $vsWhere = Get-VsWherePath
    if (-not $vsWhere) {
        return $null
    }

    $products = if ($BuildToolsOnly) {
        @("Microsoft.VisualStudio.Product.BuildTools")
    }
    else {
        @("*")
    }

    $arguments = @(
        "-latest",
        "-products"
    ) + $products + @(
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-property",
        "installationPath"
    )

    $output = & $vsWhere @arguments 2>$null
    if ($LASTEXITCODE -eq 0 -and $output) {
        return ($output | Select-Object -First 1)
    }

    return $null
}

function Get-VcVarsPath {
    $installPath = Get-VisualStudioInstallPath
    if ($installPath) {
        $vcVars = Join-Path $installPath "VC\Auxiliary\Build\vcvars64.bat"
        if (Test-Path $vcVars) {
            return $vcVars
        }
    }

    $roots = @(
        (Join-Path (Get-ProgramFilesX86) "Microsoft Visual Studio\2022"),
        (Join-Path (Get-ProgramFilesX86) "Microsoft Visual Studio\2019"),
        (Join-Path $env:ProgramFiles "Microsoft Visual Studio\2022"),
        (Join-Path $env:ProgramFiles "Microsoft Visual Studio\2019")
    )
    $editions = @("BuildTools", "Community", "Professional", "Enterprise")

    foreach ($root in $roots) {
        foreach ($edition in $editions) {
            $candidate = Join-Path $root "$edition\VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path $candidate) {
                return $candidate
            }
        }
    }

    return $null
}

function Import-MsvcEnvironment {
    $vcVars = Get-VcVarsPath
    if (-not $vcVars) {
        return $false
    }

    Write-Host "Loading Visual C++ x64 build environment..."
    $environmentLines = & cmd.exe /s /c "`"$vcVars`" >nul && set"
    if ($LASTEXITCODE -ne 0) {
        return $false
    }

    foreach ($line in $environmentLines) {
        if ($line -match '^([^=]+)=(.*)$') {
            [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], "Process")
        }
    }

    return [bool](Get-Command link.exe -ErrorAction SilentlyContinue)
}

function Test-MsvcLinker {
    if (Get-Command link.exe -ErrorAction SilentlyContinue) {
        return $true
    }

    return (Import-MsvcEnvironment)
}

function Update-VisualCppBuildTools {
    if ($NoUpdate -or $NoBuildTools -or -not $AllowDownloads) {
        return
    }

    $installPath = Get-VisualStudioInstallPath -BuildToolsOnly
    $setupExe = Join-Path (Get-ProgramFilesX86) "Microsoft Visual Studio\Installer\setup.exe"
    if ($installPath -and (Test-Path $setupExe)) {
        if (-not (Test-Administrator)) {
            Write-Warning "Skipping Visual Studio Build Tools update because PowerShell is not elevated."
            return
        }

        Invoke-CheckedProcess `
            -FilePath $setupExe `
            -ArgumentList @("update", "--installPath", $installPath, "--quiet", "--norestart") `
            -Description "Updating Visual Studio Build Tools"
    }
}

function Install-VisualCppBuildTools {
    if ($NoBuildTools) {
        throw "MSVC linker link.exe is missing. Install Visual Studio Build Tools with the Visual C++ tools, or rerun without -NoBuildTools."
    }
    if (-not $AllowDownloads) {
        throw "MSVC linker link.exe is missing. Install Visual Studio Build Tools manually, or rerun with -AllowDownloads."
    }
    if (-not (Test-Administrator)) {
        throw "MSVC linker link.exe is missing. Open PowerShell as Administrator so this script can install Visual Studio Build Tools."
    }

    $bootstrapper = Join-Path $env:TEMP "vs_BuildTools.exe"
    $installPath = Join-Path (Get-ProgramFilesX86) "Microsoft Visual Studio\2022\BuildTools"

    Write-Host "MSVC linker not found. Installing Visual Studio Build Tools for x64 C++ builds..."
    Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vs_BuildTools.exe" -OutFile $bootstrapper
    Invoke-CheckedProcess `
        -FilePath $bootstrapper `
        -ArgumentList @(
            "--quiet",
            "--wait",
            "--norestart",
            "--nocache",
            "--installPath",
            $installPath,
            "--add",
            "Microsoft.VisualStudio.Workload.VCTools",
            "--includeRecommended"
        ) `
        -Description "Installing Visual Studio Build Tools"

    Remove-Item $bootstrapper -Force -ErrorAction SilentlyContinue
}

function Ensure-VisualCppBuildTools {
    if (Test-MsvcLinker) {
        Update-VisualCppBuildTools
        [void](Import-MsvcEnvironment)
        return
    }

    Install-VisualCppBuildTools
    if (-not (Import-MsvcEnvironment)) {
        throw "Visual Studio Build Tools were installed, but link.exe is still unavailable. Reboot Windows, open PowerShell as Administrator, and rerun this builder."
    }
}

function Ensure-Rust {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        if ((Get-Command rustup -ErrorAction SilentlyContinue) -and -not $NoUpdate) {
            Write-Host "Updating Rust stable toolchain..."
            rustup update stable
            if ($LASTEXITCODE -ne 0) {
                throw "rustup update stable failed with exit code $LASTEXITCODE"
            }
        }
    }
    else {
        if (-not $AllowDownloads) {
            throw "Rust/Cargo not found. Install Rust manually, or rerun with -AllowDownloads."
        }

        Write-Host "Rust/Cargo not found. Installing Rust with rustup..."
        $rustupInit = Join-Path $env:TEMP "rustup-init.exe"
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit
        & $rustupInit -y
        if ($LASTEXITCODE -ne 0) {
            throw "rustup-init failed with exit code $LASTEXITCODE"
        }

        $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
        $env:Path = "$env:Path;$cargoBin"
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
            throw "Rust installed, but cargo is still not on PATH. Open a new PowerShell window and rerun this script."
        }
    }

    if (Get-Command rustup -ErrorAction SilentlyContinue) {
        rustup default stable
        if ($LASTEXITCODE -ne 0) {
            throw "rustup default stable failed with exit code $LASTEXITCODE"
        }
        $installedTargets = rustup target list --installed
        if ($LASTEXITCODE -ne 0) {
            throw "rustup target list --installed failed with exit code $LASTEXITCODE"
        }
        if ($installedTargets -notcontains $RustTarget) {
            if (-not $AllowDownloads) {
                throw "Rust target $RustTarget is missing. Add it manually with rustup, or rerun with -AllowDownloads."
            }
            rustup target add $RustTarget
            if ($LASTEXITCODE -ne 0) {
                throw "rustup target add $RustTarget failed with exit code $LASTEXITCODE"
            }
        }
    }
}

function Get-DotNetInstallDir {
    if ($env:LOCALAPPDATA) {
        return (Join-Path $env:LOCALAPPDATA "Microsoft\dotnet")
    }
    if ($env:USERPROFILE) {
        return (Join-Path $env:USERPROFILE ".dotnet")
    }

    return (Join-Path $env:TEMP "dotnet")
}

function Get-DotNetToolsDir {
    if ($env:USERPROFILE) {
        return (Join-Path $env:USERPROFILE ".dotnet\tools")
    }

    return $null
}

function Add-ProcessPathEntry {
    param([string]$PathEntry)

    if ([string]::IsNullOrWhiteSpace($PathEntry)) {
        return
    }

    $entries = @($env:Path -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    foreach ($entry in $entries) {
        if ($entry.TrimEnd('\').Equals($PathEntry.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) {
            return
        }
    }

    $env:Path = "$PathEntry;$env:Path"
}

function Test-DotNetSdk {
    if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
        return $false
    }

    $sdks = dotnet --list-sdks 2>$null
    return ($LASTEXITCODE -eq 0 -and $sdks)
}

function Ensure-DotNetSdk {
    $installDir = Get-DotNetInstallDir
    Add-ProcessPathEntry -PathEntry $installDir
    Add-ProcessPathEntry -PathEntry (Get-DotNetToolsDir)
    [Environment]::SetEnvironmentVariable("DOTNET_ROOT", $installDir, "Process")

    if (Test-DotNetSdk) {
        return $true
    }

    if (-not $AllowDownloads) {
        Write-Warning ".NET SDK not found; skipping MSI packaging. Rerun with -AllowDownloads if you want this script to install .NET."
        return $false
    }

    Write-Host ".NET SDK not found. Installing latest .NET LTS SDK for this build session..."
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null

    $dotnetInstall = Join-Path $env:TEMP "dotnet-install.ps1"
    Invoke-WebRequest -Uri "https://dot.net/v1/dotnet-install.ps1" -OutFile $dotnetInstall
    Unblock-File $dotnetInstall -ErrorAction SilentlyContinue
    & $dotnetInstall -Channel LTS -Architecture x64 -InstallDir $installDir
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet-install.ps1 failed with exit code $LASTEXITCODE"
    }

    Add-ProcessPathEntry -PathEntry $installDir
    [Environment]::SetEnvironmentVariable("DOTNET_ROOT", $installDir, "Process")

    if (-not (Test-DotNetSdk)) {
        throw ".NET SDK installation completed, but dotnet --list-sdks still failed."
    }

    return $true
}

function Ensure-Wix {
    if ($NoWix) {
        return $false
    }

    if (-not (Ensure-DotNetSdk)) {
        return $false
    }

    if (Get-Command wix -ErrorAction SilentlyContinue) {
        if (-not $NoUpdate -and $AllowDownloads) {
            dotnet tool update --global wix
            if ($LASTEXITCODE -ne 0) {
                throw "dotnet tool update --global wix failed with exit code $LASTEXITCODE"
            }
        }
    }
    else {
        if (-not $AllowDownloads) {
            Write-Warning "WiX not found; skipping MSI packaging. Rerun with -AllowDownloads if you want this script to install WiX."
            return $false
        }

        dotnet tool install --global wix
        if ($LASTEXITCODE -ne 0) {
            throw "dotnet tool install --global wix failed with exit code $LASTEXITCODE"
        }

        Add-ProcessPathEntry -PathEntry (Get-DotNetToolsDir)
    }

    return [bool](Get-Command wix -ErrorAction SilentlyContinue)
}

function Get-WixVersion {
    $versionOutput = & wix --version 2>$null
    if ($LASTEXITCODE -eq 0 -and $versionOutput -match '^(\d+\.\d+\.\d+)') {
        return $Matches[1]
    }

    return $null
}

function Invoke-WixEulaAcceptance {
    param(
        [AllowNull()][string]$Version,
        [bool]$AcceptEula
    )

    if (-not $AcceptEula -or $Version -notmatch '^7\.') {
        return
    }

    $output = & wix eula accept wix7 2>&1
    if ($LASTEXITCODE -ne 0) {
        $message = if ($output) { ($output -join " ") } else { "exit code $LASTEXITCODE" }
        Write-Warning "Unable to persist WiX v7 EULA acceptance: $message"
    }
}

function Test-WixExtensionCached {
    param([string]$ExtensionName)

    $extensions = & wix extension list -g 2>$null
    return ($LASTEXITCODE -eq 0 -and ($extensions -match [regex]::Escape($ExtensionName)))
}

function Add-WixExtension {
    param(
        [string]$ExtensionName,
        [string]$ExtensionRef
    )

    $output = & wix extension add -g $ExtensionRef 2>&1
    if ($LASTEXITCODE -eq 0 -or (Test-WixExtensionCached -ExtensionName $ExtensionName)) {
        return $true
    }

    $message = if ($output) { ($output -join " ") } else { "exit code $LASTEXITCODE" }
    Write-Warning "wix extension add -g $ExtensionRef failed: $message"
    return $false
}

function Ensure-WixBootstrapperExtension {
    $version = Get-WixVersion
    $legacyBootstrapperExtension = "WixToolset.Bal.wixext"
    $extensionRefs = @()
    if ($version) {
        $extensionRefs += "$WixBootstrapperExtension/$version"
    }
    $extensionRefs += $WixBootstrapperExtension
    if ($version) {
        $extensionRefs += "$legacyBootstrapperExtension/$version"
    }
    $extensionRefs += $legacyBootstrapperExtension

    foreach ($extensionRef in $extensionRefs) {
        $extensionName = ($extensionRef -split '/', 2)[0]
        if (Test-WixExtensionCached -ExtensionName $extensionName) {
            return $extensionName
        }
    }

    if (-not $AllowDownloads) {
        Write-Warning "WiX bootstrapper extension not found; skipping EXE installer packaging. Rerun with -AllowDownloads if you want this script to install the extension."
        return $null
    }

    Invoke-WixEulaAcceptance -Version $version -AcceptEula:$AcceptWixEula
    foreach ($extensionRef in $extensionRefs) {
        $extensionName = ($extensionRef -split '/', 2)[0]
        if (Add-WixExtension -ExtensionName $extensionName -ExtensionRef $extensionRef) {
            return $extensionName
        }
    }

    Write-Warning "WiX bootstrapper extension could not be installed; skipped $ArtifactBaseName.exe. The MSI installer was still created."
    return $null
}

function New-StageDirectory {
    param(
        [string]$StageDir,
        [string]$SourceExe,
        [string]$LogoPng,
        [string]$LogoIco
    )

    Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "$AppName\bin") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "$AppName\docs") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "$AppName\assets") | Out-Null
    Copy-Item $SourceExe (Join-Path $StageDir "$AppName\bin\$AppName.exe") -Force
    Copy-Item (Join-Path $RepoDir "README.md") (Join-Path $StageDir "$AppName\docs\README.md") -Force
    Copy-Item $LogoPng (Join-Path $StageDir "$AppName\assets\v_fs_backup_logo.png") -Force
    Copy-Item $LogoIco (Join-Path $StageDir "$AppName\assets\v_fs_backup_logo.ico") -Force

    @(
        '$ErrorActionPreference = "Stop"',
        "& `"`$PSScriptRoot\bin\$AppName.exe`" @args"
    ) | Set-Content -Encoding UTF8 -Path (Join-Path $StageDir "$AppName\$AppName.ps1")

    @(
        "@echo off",
        "`"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`" -NoLogo -NoExit -ExecutionPolicy Bypass -File `"%~dp0$AppName.ps1`" %*"
    ) | Set-Content -Encoding ASCII -Path (Join-Path $StageDir "$AppName\$AppName.cmd")
}

function Assert-PortableWindowsBinary {
    param([string]$Executable)

    $dependencyInspector = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    $inspectorArguments = @("/DEPENDENTS", $Executable)
    if (-not $dependencyInspector) {
        $dependencyInspector = Get-Command link.exe -ErrorAction SilentlyContinue
        $inspectorArguments = @("/DUMP", "/DEPENDENTS", $Executable)
    }

    if (-not $dependencyInspector) {
        throw "Unable to verify Windows runtime portability because dumpbin.exe/link.exe is not on PATH. Run this script from a Visual Studio Developer PowerShell."
    }

    $dependencies = & $dependencyInspector.Source @inspectorArguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to inspect runtime dependencies for $Executable.`n$($dependencies -join [Environment]::NewLine)"
    }

    $forbiddenRuntime = $dependencies | Where-Object {
        $_ -match '(?i)\b(?:VCRUNTIME|MSVCP|CONCRT)\d+(?:_\d+)?(?:D)?\.DLL\b'
    }
    if ($forbiddenRuntime) {
        throw "The release binary dynamically depends on the Visual C++ runtime and will fail on a clean Windows installation:`n$($forbiddenRuntime -join [Environment]::NewLine)"
    }

    Write-Host "verified standalone Windows runtime: no Visual C++ Redistributable DLL is required"
}

function ConvertTo-WixAttribute {
    param([AllowNull()][string]$Value)

    if ($null -eq $Value) {
        return ""
    }

    return [System.Security.SecurityElement]::Escape($Value)
}

function Get-MsiVersion {
    param([string]$Version)

    if ($Version -match '^(\d+)\.(\d+)\.(\d+)') {
        return "$($Matches[1]).$($Matches[2]).$($Matches[3])"
    }

    return "0.0.0"
}

function Build-Msi {
    param(
        [string]$OutDir,
        [string]$StageDir,
        [string]$ArtifactBaseName,
        [string]$Version,
        [string]$LogoIco,
        [bool]$AcceptEula
    )

    $wxs = Join-Path $OutDir "$ArtifactBaseName.wxs"
    $msi = Join-Path $OutDir "$ArtifactBaseName.msi"
    $msiVersion = Get-MsiVersion -Version $Version
    $payloadRoot = Join-Path $StageDir $AppName
    $payloadExe = Join-Path $payloadRoot "bin\$AppName.exe"
    $payloadLauncherPs1 = Join-Path $payloadRoot "$AppName.ps1"
    $payloadLauncherCmd = Join-Path $payloadRoot "$AppName.cmd"
    $payloadReadme = Join-Path $payloadRoot "docs\README.md"
    $payloadLogoPng = Join-Path $payloadRoot "assets\v_fs_backup_logo.png"
    $payloadLogoIco = Join-Path $payloadRoot "assets\v_fs_backup_logo.ico"
    $appNameAttr = ConvertTo-WixAttribute $AppName
    $publisherAttr = ConvertTo-WixAttribute $Publisher
    $payloadExeAttr = ConvertTo-WixAttribute $payloadExe
    $payloadLauncherPs1Attr = ConvertTo-WixAttribute $payloadLauncherPs1
    $payloadLauncherCmdAttr = ConvertTo-WixAttribute $payloadLauncherCmd
    $payloadReadmeAttr = ConvertTo-WixAttribute $payloadReadme
    $payloadLogoPngAttr = ConvertTo-WixAttribute $payloadLogoPng
    $payloadLogoIcoAttr = ConvertTo-WixAttribute $payloadLogoIco
    $logoIcoAttr = ConvertTo-WixAttribute $LogoIco

    @"
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="$appNameAttr" Manufacturer="$publisherAttr" Version="$msiVersion" UpgradeCode="{A11A41D9-6B76-4A3E-A286-15A5543B21D8}" Scope="perMachine">
    <MajorUpgrade AllowSameVersionUpgrades="yes" Schedule="afterInstallValidate" DowngradeErrorMessage="A newer version of $appNameAttr is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <Property Id="ARPPRODUCTICON" Value="VFsBackupIcon.ico" />
    <Feature Id="MainFeature" Title="$appNameAttr" Level="1">
      <ComponentGroupRef Id="AppComponents" />
    </Feature>
  </Package>

  <Fragment>
    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="$appNameAttr">
        <Directory Id="BinFolder" Name="bin" />
        <Directory Id="DocsFolder" Name="docs" />
        <Directory Id="AssetsFolder" Name="assets" />
      </Directory>
    </StandardDirectory>
    <StandardDirectory Id="ProgramMenuFolder">
      <Directory Id="ApplicationProgramsFolder" Name="$appNameAttr" />
    </StandardDirectory>
  </Fragment>

  <Fragment>
    <Icon Id="VFsBackupIcon.ico" SourceFile="$logoIcoAttr" />
    <ComponentGroup Id="AppComponents">
      <Component Id="VFsBackupExe" Directory="BinFolder" Guid="{087DE61D-6DB5-4C0C-BD02-8B46194B021C}">
        <File Id="VFsBackupExeFile" Source="$payloadExeAttr" Name="$appNameAttr.exe" KeyPath="yes" />
        <Environment Id="PathEnv" Name="PATH" Value="[BinFolder]" Action="set" Part="last" System="yes" Permanent="no" />
      </Component>
      <Component Id="VFsBackupLauncherPs1" Directory="INSTALLFOLDER" Guid="{D0E0CBCF-38CA-4F56-B8EE-F85782A7EACC}">
        <File Id="VFsBackupLauncherPs1File" Source="$payloadLauncherPs1Attr" Name="$appNameAttr.ps1" KeyPath="yes" />
      </Component>
      <Component Id="VFsBackupLauncherCmd" Directory="INSTALLFOLDER" Guid="{25458ABA-3720-473D-8FBC-D70E50DC471E}">
        <File Id="VFsBackupLauncherCmdFile" Source="$payloadLauncherCmdAttr" Name="$appNameAttr.cmd" KeyPath="yes" />
      </Component>
      <Component Id="ReadmeDoc" Directory="DocsFolder" Guid="{7C5F75D9-B1FF-43D4-9CE5-5E6E213C90C2}">
        <File Id="ReadmeFile" Source="$payloadReadmeAttr" Name="README.md" KeyPath="yes" />
      </Component>
      <Component Id="LogoAssets" Directory="AssetsFolder" Guid="{720FDBF8-E9BC-42E8-BF84-3B9D5E140AB1}">
        <File Id="LogoPngFile" Source="$payloadLogoPngAttr" Name="v_fs_backup_logo.png" />
        <File Id="LogoIcoFile" Source="$payloadLogoIcoAttr" Name="v_fs_backup_logo.ico" KeyPath="yes" />
      </Component>
      <Component Id="StartMenuShortcut" Directory="ApplicationProgramsFolder" Guid="{FB9C3CAF-16ED-4F24-8B6F-AC6A95F4913C}">
        <Shortcut Id="StartMenuShortcut" Name="$appNameAttr" Description="Open $appNameAttr in PowerShell" Target="[#VFsBackupLauncherCmdFile]" WorkingDirectory="INSTALLFOLDER" Icon="VFsBackupIcon.ico" IconIndex="0" />
        <RemoveFolder Id="RemoveStartMenuFolder" On="uninstall" />
        <RegistryValue Root="HKLM" Key="Software\$publisherAttr\$appNameAttr" Name="installed" Type="integer" Value="1" KeyPath="yes" />
      </Component>
      <Component Id="FsbFileAssociation" Directory="INSTALLFOLDER" Guid="{4A52BB38-7A3C-4DD3-8BD7-E3E62877866E}">
        <RegistryValue Root="HKLM" Key="Software\Classes\.fsb" Value="$appNameAttr.fsb" Type="string" KeyPath="yes" />
        <RegistryValue Root="HKLM" Key="Software\Classes\.fsb" Name="Content Type" Value="application/x-v-fs-backup" Type="string" />
        <RegistryValue Root="HKLM" Key="Software\Classes\$appNameAttr.fsb" Value="v_fs_backup archive" Type="string" />
        <RegistryValue Root="HKLM" Key="Software\Classes\$appNameAttr.fsb\DefaultIcon" Value="[AssetsFolder]v_fs_backup_logo.ico,0" Type="string" />
        <RegistryValue Root="HKLM" Key="Software\Classes\$appNameAttr.fsb\shell\open\command" Value="&quot;[INSTALLFOLDER]$appNameAttr.cmd&quot;" Type="string" />
      </Component>
    </ComponentGroup>
  </Fragment>
</Wix>
"@ | Set-Content -Encoding UTF8 -Path $wxs

    $msiBuilt = $false
    Push-Location $OutDir
    try {
        $wixArgs = @("build", "$ArtifactBaseName.wxs", "-arch", "x64", "-out", "$ArtifactBaseName.msi")
        if ($AcceptEula) {
            $wixArgs = @("build", "-acceptEula", "wix7", "$ArtifactBaseName.wxs", "-arch", "x64", "-out", "$ArtifactBaseName.msi")
        }

        & wix @wixArgs
        if ($LASTEXITCODE -ne 0) {
            if (-not $AcceptEula) {
                Write-Warning "Skipped MSI packaging because WiX v7 requires explicit OSMF EULA acceptance. Rerun with -AcceptWixEula only if you accept the WiX v7 EULA."
            }
            else {
                throw "wix build failed with exit code $LASTEXITCODE"
            }
        }
        else {
            $msiBuilt = $true
        }
    }
    finally {
        Pop-Location
    }

    Remove-Item $wxs -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $OutDir "$ArtifactBaseName.wixpdb") -Force -ErrorAction SilentlyContinue
    if (-not $msiBuilt) {
        Remove-Item $msi -Force -ErrorAction SilentlyContinue
        return $false
    }

    Write-Host "packaged $msi"
    return $true
}

function Build-ExeInstaller {
    param(
        [string]$OutDir,
        [string]$ArtifactBaseName,
        [string]$Version,
        [string]$LogoPng,
        [string]$LogoIco,
        [string]$LogoSplash,
        [bool]$AcceptEula,
        [string]$WixExtensionRef
    )

    $bundleWxs = Join-Path $OutDir "$ArtifactBaseName.bundle.wxs"
    $bundleExe = Join-Path $OutDir "$ArtifactBaseName.exe"
    $msi = Join-Path $OutDir "$ArtifactBaseName.msi"
    if (-not (Test-Path $msi)) {
        return $false
    }

    $msiVersion = Get-MsiVersion -Version $Version
    $appNameAttr = ConvertTo-WixAttribute $AppName
    $publisherAttr = ConvertTo-WixAttribute $Publisher
    $msiAttr = ConvertTo-WixAttribute $msi
    $logoPngAttr = ConvertTo-WixAttribute $LogoPng
    $logoIcoAttr = ConvertTo-WixAttribute $LogoIco
    $logoSplashAttr = ConvertTo-WixAttribute $LogoSplash

    @"
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs" xmlns:bal="http://wixtoolset.org/schemas/v4/wxs/bal">
  <Bundle Name="$appNameAttr" Manufacturer="$publisherAttr" Version="$msiVersion" UpgradeCode="{1D13CD7A-448A-42CD-A407-24DD951B29D6}" Compressed="yes" IconSourceFile="$logoIcoAttr" SplashScreenSourceFile="$logoSplashAttr">
    <BootstrapperApplication>
      <bal:WixStandardBootstrapperApplication LicenseUrl="" LogoFile="$logoPngAttr" ShowVersion="yes" Theme="hyperlinkLicense" />
    </BootstrapperApplication>
    <Chain>
      <MsiPackage SourceFile="$msiAttr" Visible="no" ForcePerMachine="yes" Compressed="yes" />
    </Chain>
  </Bundle>
</Wix>
"@ | Set-Content -Encoding UTF8 -Path $bundleWxs

    $exeBuilt = $false
    Push-Location $OutDir
    try {
        $wixArgs = @("build", "$ArtifactBaseName.bundle.wxs", "-arch", "x64", "-ext", $WixExtensionRef, "-out", "$ArtifactBaseName.exe")
        if ($AcceptEula) {
            $wixArgs = @("build", "-acceptEula", "wix7", "$ArtifactBaseName.bundle.wxs", "-arch", "x64", "-ext", $WixExtensionRef, "-out", "$ArtifactBaseName.exe")
        }

        & wix @wixArgs
        if ($LASTEXITCODE -ne 0) {
            if (-not $AcceptEula) {
                Write-Warning "Skipped EXE installer packaging because WiX v7 requires explicit OSMF EULA acceptance. Rerun with -AcceptWixEula only if you accept the WiX v7 EULA."
            }
            else {
                throw "wix build for EXE installer failed with exit code $LASTEXITCODE"
            }
        }
        else {
            $exeBuilt = $true
        }
    }
    finally {
        Pop-Location
    }

    Remove-Item $bundleWxs -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $OutDir "$ArtifactBaseName.bundle.wixpdb") -Force -ErrorAction SilentlyContinue
    if (-not $exeBuilt) {
        Remove-Item $bundleExe -Force -ErrorAction SilentlyContinue
        return $false
    }

    Write-Host "packaged $bundleExe"
    return $true
}

function Write-Checksums {
    param(
        [string]$OutDir,
        [string]$ArtifactBaseName,
        [string[]]$ArtifactNames
    )

    $checksums = Join-Path $OutDir "$ArtifactBaseName.sha256"
    Remove-Item $checksums -Force -ErrorAction SilentlyContinue
    foreach ($artifactName in $ArtifactNames) {
        $artifact = Join-Path $OutDir $artifactName
        if (Test-Path $artifact) {
            $hash = (Get-FileHash -Algorithm SHA256 -Path $artifact).Hash.ToLowerInvariant()
            Add-Content -Path $checksums -Value "$hash  $artifactName"
        }
    }
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoDir = (Resolve-Path (Join-Path $ScriptDir "..")).ProviderPath
$Version = Get-PackageVersion
$VersionedName = "${AppName}_v${Version}"
$ArtifactBaseName = "${VersionedName}_windows_x86_64"
$OutDir = Join-Path $RepoDir "versions\$VersionedName"
$StageDir = Join-Path $OutDir ".stage-windows"
$SourceExe = Join-Path $RepoDir "target\$RustTarget\release\$AppName.exe"
$OutputZip = Join-Path $OutDir "$ArtifactBaseName.zip"
$LogoPng = Join-Path $RepoDir "assets\v_fs_backup_logo_256.png"
$LogoIco = Join-Path $RepoDir "assets\v_fs_backup_logo.ico"
$LogoSplash = Join-Path $RepoDir "assets\v_fs_backup_logo_splash.bmp"

foreach ($asset in @($LogoPng, $LogoIco, $LogoSplash)) {
    if (-not (Test-Path $asset)) {
        throw "Missing logo asset at $asset. Run scripts\prepare_logo_assets.py first."
    }
}

$CodeSigningCertificate = Resolve-CodeSigningCertificate

Ensure-Rust
Ensure-VisualCppBuildTools

if (-not $NoUpdate) {
    Push-Location $RepoDir
    try {
        cargo update
        if ($LASTEXITCODE -ne 0) {
            throw "cargo update failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

$cargoArgs = @("build", "--release", "--target", $RustTarget)
if ($Locked) {
    $cargoArgs += "--locked"
}

Push-Location $RepoDir
try {
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path $SourceExe)) {
    throw "Release binary not found at $SourceExe"
}

Assert-PortableWindowsBinary -Executable $SourceExe

Invoke-CodeSignFile `
    -Certificate $CodeSigningCertificate `
    -FilePath $SourceExe `
    -Description "$AppName release binary"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem -Path $OutDir -Filter "$ArtifactBaseName*" -File -ErrorAction SilentlyContinue | Remove-Item -Force

New-StageDirectory -StageDir $StageDir -SourceExe $SourceExe -LogoPng $LogoPng -LogoIco $LogoIco
Compress-Archive -Path (Join-Path $StageDir $AppName) -DestinationPath $OutputZip -Force
Write-Host "packaged $OutputZip"

$MsiCreated = $false
$ExeCreated = $false
if (Ensure-Wix) {
    $MsiCreated = Build-Msi `
        -OutDir $OutDir `
        -StageDir $StageDir `
        -ArtifactBaseName $ArtifactBaseName `
        -Version $Version `
        -LogoIco $LogoIco `
        -AcceptEula:$AcceptWixEula

    if ($MsiCreated) {
        Invoke-CodeSignFile `
            -Certificate $CodeSigningCertificate `
            -FilePath (Join-Path $OutDir "$ArtifactBaseName.msi") `
            -Description "$ArtifactBaseName.msi"

        $wixExtensionRef = Ensure-WixBootstrapperExtension
        if ($wixExtensionRef) {
            $ExeCreated = Build-ExeInstaller `
                -OutDir $OutDir `
                -ArtifactBaseName $ArtifactBaseName `
                -Version $Version `
                -LogoPng $LogoPng `
                -LogoIco $LogoIco `
                -LogoSplash $LogoSplash `
                -AcceptEula:$AcceptWixEula `
                -WixExtensionRef $wixExtensionRef

            if ($ExeCreated) {
                Invoke-CodeSignBundle `
                    -Certificate $CodeSigningCertificate `
                    -BundlePath (Join-Path $OutDir "$ArtifactBaseName.exe") `
                    -Description "$ArtifactBaseName.exe"
            }
        }
        else {
            Write-Warning "Skipped $ArtifactBaseName.exe"
        }
    }
    else {
        Write-Warning "Skipped $ArtifactBaseName.exe because MSI packaging did not complete."
    }
}
else {
    Write-Warning "Skipped $ArtifactBaseName.msi"
    Write-Warning "Skipped $ArtifactBaseName.exe"
}

$ArtifactNames = @("$ArtifactBaseName.zip")
if ($ExeCreated) {
    $ArtifactNames += "$ArtifactBaseName.exe"
}
if ($MsiCreated) {
    $ArtifactNames += "$ArtifactBaseName.msi"
}

Write-Checksums `
    -OutDir $OutDir `
    -ArtifactBaseName $ArtifactBaseName `
    -ArtifactNames $ArtifactNames

Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue

if ($NeedsReboot) {
    Write-Warning "A Windows reboot is required before every terminal can see the updated build tools."
}

if ($UsingGeneratedCodeSigningCertificate) {
    Write-Warning "These artifacts were signed with the generated local self-signed certificate. They are trusted only on machines where certs\$AppName-$($ExpectedSignaturePublisher -replace '[^A-Za-z0-9._-]', '_')-local-codesign.cer is installed as trusted."
}

Write-Host "Windows artifacts created under $OutDir" -ForegroundColor Green
Write-Host ""
