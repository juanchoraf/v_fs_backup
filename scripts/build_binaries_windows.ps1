#requires -version 5.1
[CmdletBinding()]
param(
    [switch]$Locked,
    [switch]$NoUpdate,
    [switch]$NoDownloads,
    [switch]$NoWix,
    [switch]$AcceptWixEula,
    [switch]$NoWixEula,
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
$CliBin = "v_fs_backup"
$Publisher = "TheVelasquez.com"
$ExpectedSignaturePublisher = "TheVelasquez.com"
$RustTarget = "x86_64-pc-windows-msvc"
$MsiUpgradeCode = "{2E35712F-13E1-44B1-A802-4FE8F14D387C}"
$BundleUpgradeCode = "{38BCBB40-9F2F-4418-9F3C-33A39E2EB29F}"
$WixBootstrapperExtension = "WixToolset.BootstrapperApplications.wixext"
$NeedsReboot = $false
$UsingGeneratedCodeSigningCertificate = $false
$RunningInGitHubActions = $env:GITHUB_ACTIONS -eq "true"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Set-BuildPrivacy {
    $values = [ordered]@{
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
    foreach ($name in $values.Keys) {
        [Environment]::SetEnvironmentVariable($name, $values[$name], "Process")
        Set-Item -Path "Env:$name" -Value $values[$name]
    }
}

function Add-ProcessPathEntry {
    param([string]$PathEntry)
    if ([string]::IsNullOrWhiteSpace($PathEntry)) { return }
    $current = @($env:Path -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    foreach ($entry in $current) {
        if ($entry.TrimEnd('\').Equals($PathEntry.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) { return }
    }
    $env:Path = "$PathEntry;$env:Path"
}

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-PackageVersion {
    $cargoToml = Join-Path $RepoDir "Cargo.toml"
    foreach ($line in (Get-Content $cargoToml)) {
        if ($line -match '^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
    }
    throw "Unable to read package version from $cargoToml"
}

function Get-MsiVersion {
    param([string]$Version)
    if ($Version -match '^(\d+)\.(\d+)\.(\d+)') { return "$($Matches[1]).$($Matches[2]).$($Matches[3])" }
    return "0.0.0"
}

function ConvertTo-WixAttribute {
    param([AllowNull()][string]$Value)
    if ($null -eq $Value) { return "" }
    return [System.Security.SecurityElement]::Escape($Value)
}

function Get-CertificatePaths {
    $certsDir = Join-Path $RepoDir "certs"
    $safePublisher = $ExpectedSignaturePublisher -replace '[^A-Za-z0-9._-]', '_'
    $base = "$AppName-$safePublisher-local-codesign"
    return [pscustomobject]@{
        Directory = $certsDir
        Cer = Join-Path $certsDir "$base.cer"
        Pfx = Join-Path $certsDir "$base.pfx"
        Password = Join-Path $certsDir "$base.password.txt"
        Thumbprint = Join-Path $certsDir "$base.thumbprint.txt"
    }
}

function Test-CodeSigningCertificate {
    param([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)
    if ($null -eq $Certificate -or -not $Certificate.HasPrivateKey) { return $false }
    $now = Get-Date
    if ($Certificate.NotBefore -gt $now -or $Certificate.NotAfter -le $now) { return $false }
    $simpleName = $Certificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
    return $simpleName.Equals($ExpectedSignaturePublisher, [System.StringComparison]::OrdinalIgnoreCase)
}

function New-CertificatePassword {
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    return [Convert]::ToBase64String($bytes)
}

function Export-GeneratedCertificate {
    param([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)
    $paths = Get-CertificatePaths
    New-Item -ItemType Directory -Force -Path $paths.Directory | Out-Null
    Export-Certificate -Cert $Certificate -FilePath $paths.Cer -Force | Out-Null
    Set-Content -Encoding ASCII -Path $paths.Thumbprint -Value $Certificate.Thumbprint
    $password = $GeneratedCodeSigningCertPassword
    if ([string]::IsNullOrWhiteSpace($password)) {
        if (Test-Path $paths.Password) {
            $password = (Get-Content -Path $paths.Password -Raw).Trim()
        } else {
            $password = New-CertificatePassword
            Set-Content -Encoding ASCII -Path $paths.Password -Value $password
        }
    }
    $securePassword = ConvertTo-SecureString -String $password -AsPlainText -Force
    Export-PfxCertificate -Cert $Certificate -FilePath $paths.Pfx -Password $securePassword -Force | Out-Null
}

function Import-GeneratedCertificateTrust {
    param([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)
    $paths = Get-CertificatePaths
    foreach ($store in @("Cert:\CurrentUser\Root", "Cert:\CurrentUser\TrustedPublisher")) {
        Import-Certificate -FilePath $paths.Cer -CertStoreLocation $store | Out-Null
    }
    if ($TrustGeneratedCodeSigningCertificateForAllUsers) {
        if (-not (Test-Administrator)) { throw "Trusting the generated certificate for all users requires Administrator PowerShell." }
        foreach ($store in @("Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher")) {
            Import-Certificate -FilePath $paths.Cer -CertStoreLocation $store | Out-Null
        }
    }
}

function Get-OrCreateGeneratedCertificate {
    if ($NoGenerateCodeSigningCertificate) {
        throw "A code signing certificate is required. Set V_FS_BACKUP_CODESIGN_* or allow local generation."
    }
    $friendlyName = "$AppName local code signing ($ExpectedSignaturePublisher)"
    $certificate = Get-ChildItem -Path "Cert:\CurrentUser\My" -ErrorAction SilentlyContinue |
        Where-Object { $_.FriendlyName -eq $friendlyName -and (Test-CodeSigningCertificate -Certificate $_) } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
    if (-not $certificate) {
        Write-Warning "Creating a local self-signed code signing certificate for $ExpectedSignaturePublisher."
        $certificate = New-SelfSignedCertificate `
            -Type CodeSigningCert `
            -Subject "CN=$ExpectedSignaturePublisher" `
            -FriendlyName $friendlyName `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -KeyAlgorithm RSA `
            -KeyLength 3072 `
            -HashAlgorithm SHA256 `
            -KeyExportPolicy Exportable `
            -NotAfter (Get-Date).AddYears(3)
    }
    if (-not (Test-CodeSigningCertificate -Certificate $certificate)) {
        throw "Generated code signing certificate is not valid for $ExpectedSignaturePublisher."
    }
    Export-GeneratedCertificate -Certificate $certificate
    if (-not $RunningInGitHubActions) { Import-GeneratedCertificateTrust -Certificate $certificate }
    $script:UsingGeneratedCodeSigningCertificate = $true
    return $certificate
}

function Resolve-CodeSigningCertificate {
    if ($SkipCodeSigning) {
        Write-Warning "Building unsigned Windows artifacts. UAC will show Publisher: Unknown."
        return $null
    }
    if ($RunningInGitHubActions -and [string]::IsNullOrWhiteSpace($CodeSigningCertPath) -and [string]::IsNullOrWhiteSpace($CodeSigningCertThumbprint)) {
        Write-Warning "No V_FS_BACKUP_CODESIGN_* secret configured. Building unsigned Windows artifacts in GitHub Actions."
        return $null
    }
    if (-not [string]::IsNullOrWhiteSpace($CodeSigningCertThumbprint)) {
        $thumb = ($CodeSigningCertThumbprint -replace '\s', '').ToUpperInvariant()
        foreach ($store in @("Cert:\CurrentUser\My", "Cert:\LocalMachine\My")) {
            $cert = Get-ChildItem -Path $store -ErrorAction SilentlyContinue |
                Where-Object { ($_.Thumbprint -replace '\s', '').ToUpperInvariant() -eq $thumb } |
                Select-Object -First 1
            if ($cert) {
                if (-not (Test-CodeSigningCertificate -Certificate $cert)) { throw "Code signing certificate is not valid for $ExpectedSignaturePublisher." }
                return $cert
            }
        }
        throw "Unable to find code signing certificate thumbprint $CodeSigningCertThumbprint."
    }
    if (-not [string]::IsNullOrWhiteSpace($CodeSigningCertPath)) {
        $resolved = (Resolve-Path $CodeSigningCertPath).Path
        $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable -bor [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::PersistKeySet -bor [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::UserKeySet
        $cert = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($resolved, $CodeSigningCertPassword, $flags)
        if (-not (Test-CodeSigningCertificate -Certificate $cert)) { throw "Code signing certificate at $resolved is not valid for $ExpectedSignaturePublisher." }
        return $cert
    }
    return Get-OrCreateGeneratedCertificate
}

function Invoke-CodeSignFile {
    param([AllowNull()][System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate, [string]$FilePath, [string]$Description)
    if ($null -eq $Certificate) { return }
    $args = @{ FilePath = $FilePath; Certificate = $Certificate; HashAlgorithm = "SHA256" }
    if (-not $UsingGeneratedCodeSigningCertificate -and -not [string]::IsNullOrWhiteSpace($CodeSigningTimestampUrl)) {
        $args["TimestampServer"] = $CodeSigningTimestampUrl
    }
    $signature = Set-AuthenticodeSignature @args
    if ($signature.Status -ne "Valid") { throw "Code signing failed for $Description at $FilePath. Status: $($signature.Status)." }
    Write-Host "signed $Description as $ExpectedSignaturePublisher"
}

function Invoke-CodeSignBundle {
    param([AllowNull()][System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate, [string]$BundlePath, [string]$Description)
    if ($null -eq $Certificate) { return }
    $signDir = Join-Path $env:TEMP "$AppName-burn-sign-$([guid]::NewGuid().ToString('N'))"
    $enginePath = Join-Path $signDir "burn-engine.exe"
    $reattachedBundlePath = Join-Path $signDir "reattached-bundle.exe"
    New-Item -ItemType Directory -Force -Path $signDir | Out-Null
    try {
        & wix burn detach $BundlePath -engine $enginePath
        if ($LASTEXITCODE -ne 0) { throw "wix burn detach failed with exit code $LASTEXITCODE" }
        Invoke-CodeSignFile -Certificate $Certificate -FilePath $enginePath -Description "$Description Burn engine"
        & wix burn reattach $BundlePath -engine $enginePath -o $reattachedBundlePath
        if ($LASTEXITCODE -ne 0) { throw "wix burn reattach failed with exit code $LASTEXITCODE" }
        Invoke-CodeSignFile -Certificate $Certificate -FilePath $reattachedBundlePath -Description $Description
        Copy-Item $reattachedBundlePath $BundlePath -Force
    } finally {
        Remove-Item $signDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Ensure-RustTarget {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "Cargo is missing. Install Rust from https://rustup.rs/." }
    if (Get-Command rustup -ErrorAction SilentlyContinue) {
        rustup default stable
        if ($LASTEXITCODE -ne 0) { throw "rustup default stable failed with exit code $LASTEXITCODE" }
        $installedTargets = rustup target list --installed
        if ($LASTEXITCODE -ne 0) { throw "rustup target list --installed failed with exit code $LASTEXITCODE" }
        if ($installedTargets -notcontains $RustTarget) {
            rustup target add $RustTarget
            if ($LASTEXITCODE -ne 0) { throw "rustup target add $RustTarget failed with exit code $LASTEXITCODE" }
        }
    }
}

function Ensure-Wix {
    if ($NoWix) { return $false }
    Add-ProcessPathEntry -PathEntry (Join-Path $env:USERPROFILE ".dotnet\tools")
    if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
        if ($NoDownloads) {
            Write-Warning "WiX is missing; skipping MSI and EXE installer packaging."
            return $false
        }
        if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
            Write-Warning ".NET SDK is missing; skipping MSI and EXE installer packaging."
            return $false
        }
        dotnet tool install --global wix
        if ($LASTEXITCODE -ne 0) { throw "dotnet tool install --global wix failed with exit code $LASTEXITCODE" }
        Add-ProcessPathEntry -PathEntry (Join-Path $env:USERPROFILE ".dotnet\tools")
    }
    return [bool](Get-Command wix -ErrorAction SilentlyContinue)
}

function Get-WixAcceptArgs {
    if (-not $AcceptWixEula) { return @() }
    $versionOutput = & wix --version 2>$null
    if (($LASTEXITCODE -eq 0) -and ($versionOutput -match '^([0-9]+)\.') -and ([int]$Matches[1] -ge 7)) {
        return @("-acceptEula", "wix7")
    }
    return @()
}

function Get-WixVersion {
    $versionOutput = & wix --version 2>$null
    if ($LASTEXITCODE -eq 0 -and $versionOutput -match '^(\d+\.\d+\.\d+)') { return $Matches[1] }
    return $null
}

function Invoke-WixEulaAcceptance {
    param([AllowNull()][string]$Version)
    if ((-not $AcceptWixEula) -or ($Version -notmatch '^([0-9]+)\.') -or ([int]$Matches[1] -lt 7)) { return }
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
    param([string]$ExtensionName, [string]$ExtensionRef)
    $output = & wix extension add -g $ExtensionRef 2>&1
    if ($LASTEXITCODE -eq 0 -or (Test-WixExtensionCached -ExtensionName $ExtensionName)) { return $true }
    $message = if ($output) { ($output -join " ") } else { "exit code $LASTEXITCODE" }
    Write-Warning "wix extension add -g $ExtensionRef failed: $message"
    return $false
}

function Ensure-WixBootstrapperExtension {
    $version = Get-WixVersion
    $legacyExtension = "WixToolset.Bal.wixext"
    $extensionRefs = @()
    if ($version) { $extensionRefs += "$WixBootstrapperExtension/$version" }
    $extensionRefs += $WixBootstrapperExtension
    if ($version) { $extensionRefs += "$legacyExtension/$version" }
    $extensionRefs += $legacyExtension
    foreach ($extensionRef in $extensionRefs) {
        $extensionName = ($extensionRef -split '/', 2)[0]
        if (Test-WixExtensionCached -ExtensionName $extensionName) { return $extensionName }
    }
    if ($NoDownloads) {
        Write-Warning "WiX bootstrapper extension not found; skipping EXE installer packaging."
        return $null
    }
    Invoke-WixEulaAcceptance -Version $version
    foreach ($extensionRef in $extensionRefs) {
        $extensionName = ($extensionRef -split '/', 2)[0]
        if (Add-WixExtension -ExtensionName $extensionName -ExtensionRef $extensionRef) { return $extensionName }
    }
    Write-Warning "WiX bootstrapper extension could not be installed; skipped $ArtifactBaseName.exe. The MSI installer was still created."
    return $null
}

function New-StageDirectory {
    param([string]$StageDir, [string]$AppExe, [string]$LogoPng, [string]$IconIco)
    Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "$AppName\bin") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "$AppName\docs") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "$AppName\assets") | Out-Null
    Copy-Item $AppExe (Join-Path $StageDir "$AppName\bin\$CliBin.exe") -Force
    Copy-Item (Join-Path $RepoDir "README.md") (Join-Path $StageDir "$AppName\docs\README.md") -Force
    Copy-Item $LogoPng (Join-Path $StageDir "$AppName\assets\$AppName-logo.png") -Force
    Copy-Item $IconIco (Join-Path $StageDir "$AppName\assets\$AppName.ico") -Force
}

function Build-Msi {
    param([string]$OutDir, [string]$StageDir, [string]$ArtifactBaseName, [string]$Version, [string]$IconPath)
    $wxs = Join-Path $OutDir "$ArtifactBaseName.wxs"
    $msiVersion = Get-MsiVersion -Version $Version
    $payloadRoot = Join-Path $StageDir $AppName
    $appExe = ConvertTo-WixAttribute (Join-Path $payloadRoot "bin\$CliBin.exe")
    $readme = ConvertTo-WixAttribute (Join-Path $payloadRoot "docs\README.md")
    $logo = ConvertTo-WixAttribute (Join-Path $payloadRoot "assets\$AppName-logo.png")
    $icon = ConvertTo-WixAttribute $IconPath
    $appNameAttr = ConvertTo-WixAttribute $AppName
    $publisherAttr = ConvertTo-WixAttribute $Publisher
    $upgradeCodeAttr = ConvertTo-WixAttribute $MsiUpgradeCode
    @"
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="$appNameAttr" Manufacturer="$publisherAttr" Version="$msiVersion" UpgradeCode="$upgradeCodeAttr" Scope="perMachine">
    <MajorUpgrade AllowSameVersionUpgrades="yes" Schedule="afterInstallExecute" DowngradeErrorMessage="A newer version of $appNameAttr is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <Icon Id="AppIcon" SourceFile="$icon" />
    <Property Id="ARPPRODUCTICON" Value="AppIcon" />
    <Feature Id="MainFeature" Title="$appNameAttr" Level="1"><ComponentGroupRef Id="AppComponents" /></Feature>
  </Package>
  <Fragment>
    <StandardDirectory Id="ProgramFiles64Folder"><Directory Id="INSTALLFOLDER" Name="$appNameAttr"><Directory Id="BinFolder" Name="bin" /><Directory Id="DocsFolder" Name="docs" /><Directory Id="AssetsFolder" Name="assets" /></Directory></StandardDirectory>
    <StandardDirectory Id="ProgramMenuFolder"><Directory Id="ApplicationProgramsFolder" Name="$appNameAttr" /></StandardDirectory>
  </Fragment>
  <Fragment>
    <ComponentGroup Id="AppComponents">
      <Component Id="AppExe" Directory="BinFolder" Guid="{F9568C41-BAA5-42A1-A88E-03A9D962E850}"><File Id="AppExeFile" Source="$appExe" Name="$CliBin.exe" KeyPath="yes" /><Environment Id="PathEnv" Name="PATH" Value="[BinFolder]" Action="set" Part="last" System="yes" Permanent="no" /></Component>
      <Component Id="ReadmeDoc" Directory="DocsFolder" Guid="{40790F68-D7D7-4F4C-8CFB-F62D568EE6CB}"><File Id="ReadmeFile" Source="$readme" Name="README.md" KeyPath="yes" /></Component>
      <Component Id="LogoAsset" Directory="AssetsFolder" Guid="{7572B153-0F2D-4FD9-8F95-3E44E41FF127}"><File Id="LogoFile" Source="$logo" Name="$AppName-logo.png" KeyPath="yes" /></Component>
      <Component Id="IconAsset" Directory="AssetsFolder" Guid="{1B695B18-BCE4-4885-90B4-56D22EA30F9E}"><File Id="IconFile" Source="$icon" Name="$AppName.ico" KeyPath="yes" /></Component>
      <Component Id="StartMenuShortcut" Directory="ApplicationProgramsFolder" Guid="{A3B57A53-D42B-407B-A21B-71C2CB3837E9}"><Shortcut Id="StartMenuShortcut" Name="$appNameAttr" Description="Open $appNameAttr" Target="[#AppExeFile]" WorkingDirectory="BinFolder" Icon="AppIcon" IconIndex="0" /><RemoveFolder Id="RemoveStartMenuFolder" On="uninstall" /><RegistryValue Root="HKLM" Key="Software\$publisherAttr\$appNameAttr" Name="installed" Type="integer" Value="1" KeyPath="yes" /></Component>
      <Component Id="FsbFileAssociation" Directory="INSTALLFOLDER" Guid="{3612D12C-B355-4C09-A212-FF1658996FDE}"><RegistryValue Root="HKCR" Key=".fsb" Type="string" Value="TheVelasquez.v_fs_backup.fsb" KeyPath="yes" /><RegistryValue Root="HKCR" Key="TheVelasquez.v_fs_backup.fsb" Type="string" Value="v_fs_backup archive" /><RegistryValue Root="HKCR" Key="TheVelasquez.v_fs_backup.fsb\DefaultIcon" Type="string" Value="[AssetsFolder]$AppName.ico" /></Component>
    </ComponentGroup>
  </Fragment>
</Wix>
"@ | Set-Content -Encoding UTF8 -Path $wxs
    Push-Location $OutDir
    try {
        $wixArgs = @("build") + (Get-WixAcceptArgs) + @("$ArtifactBaseName.wxs", "-arch", "x64", "-out", "$ArtifactBaseName.msi")
        $output = & wix @wixArgs 2>&1
        if ($LASTEXITCODE -ne 0) {
            if ($output) { $output | ForEach-Object { Write-Warning $_ } }
            throw "wix build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
    Remove-Item $wxs -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $OutDir "$ArtifactBaseName.wixpdb") -Force -ErrorAction SilentlyContinue
    Write-Host "packaged $(Join-Path $OutDir "$ArtifactBaseName.msi")"
    return $true
}

function Build-ExeInstaller {
    param([string]$OutDir, [string]$ArtifactBaseName, [string]$Version, [string]$WixExtensionRef, [string]$IconPath)
    $bundleWxs = Join-Path $OutDir "$ArtifactBaseName.bundle.wxs"
    $bundleExe = Join-Path $OutDir "$ArtifactBaseName.exe"
    $msiPath = Join-Path $OutDir "$ArtifactBaseName.msi"
    if (-not (Test-Path $msiPath)) { return $false }
    $msi = ConvertTo-WixAttribute $msiPath
    $appNameAttr = ConvertTo-WixAttribute $AppName
    $publisherAttr = ConvertTo-WixAttribute $Publisher
    $upgradeCodeAttr = ConvertTo-WixAttribute $BundleUpgradeCode
    $msiVersion = Get-MsiVersion -Version $Version
    $bundleIcon = ConvertTo-WixAttribute $IconPath
    @"
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs" xmlns:bal="http://wixtoolset.org/schemas/v4/wxs/bal">
  <Bundle Name="$appNameAttr" Manufacturer="$publisherAttr" Version="$msiVersion" UpgradeCode="$upgradeCodeAttr" Compressed="yes" IconSourceFile="$bundleIcon">
    <BootstrapperApplication><bal:WixStandardBootstrapperApplication LicenseUrl="" ShowVersion="yes" Theme="hyperlinkLicense" /></BootstrapperApplication>
    <Chain><MsiPackage SourceFile="$msi" Visible="no" ForcePerMachine="yes" Compressed="yes" /></Chain>
  </Bundle>
</Wix>
"@ | Set-Content -Encoding UTF8 -Path $bundleWxs
    Push-Location $OutDir
    try {
        $wixArgs = @("build") + (Get-WixAcceptArgs) + @("$ArtifactBaseName.bundle.wxs", "-arch", "x64", "-ext", $WixExtensionRef, "-out", "$ArtifactBaseName.exe")
        $output = & wix @wixArgs 2>&1
        if ($LASTEXITCODE -ne 0) {
            if ($output) { $output | ForEach-Object { Write-Warning $_ } }
            if (-not $AcceptWixEula) {
                Write-Warning "Skipped EXE installer packaging because WiX may require explicit EULA acceptance."
                Remove-Item $bundleExe -Force -ErrorAction SilentlyContinue
                return $false
            }
            throw "wix build for EXE installer failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
    Remove-Item $bundleWxs -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $OutDir "$ArtifactBaseName.bundle.wixpdb") -Force -ErrorAction SilentlyContinue
    Write-Host "packaged $(Join-Path $OutDir "$ArtifactBaseName.exe")"
    return $true
}

function Write-Checksums {
    param([string]$OutDir, [string]$ArtifactBaseName, [string[]]$ArtifactNames)
    $checksums = Join-Path $OutDir "$ArtifactBaseName.sha256"
    Remove-Item $checksums -Force -ErrorAction SilentlyContinue
    foreach ($artifactName in $ArtifactNames) {
        $artifact = Join-Path $OutDir $artifactName
        if (Test-Path $artifact) {
            $hash = (Get-FileHash -Algorithm SHA256 -Path $artifact).Hash.ToLowerInvariant()
            Add-Content -Encoding ASCII -Path $checksums -Value "$hash  $artifactName"
        }
    }
}

Set-BuildPrivacy
if ($AcceptWixEula -and $NoWixEula) {
    throw "Use either -AcceptWixEula or -NoWixEula, not both."
}
if (-not $NoWixEula) {
    $AcceptWixEula = $true
}
Write-Host ""

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoDir = [System.IO.Path]::GetFullPath((Resolve-Path (Join-Path $ScriptDir "..")).ProviderPath)
$Version = Get-PackageVersion
$VersionedName = "${AppName}_v${Version}"
$ArtifactBaseName = "${VersionedName}_windows_x86_64"
$OutDir = Join-Path $RepoDir "versions\$VersionedName"
$StageDir = Join-Path $OutDir ".stage-windows"
$AppExe = Join-Path $RepoDir "target\$RustTarget\release\$CliBin.exe"
$OutputZip = Join-Path $OutDir "$ArtifactBaseName.zip"
$LogoPng = Join-Path $RepoDir "assets\v_fs_backup_logo.png"
$IconIco = Join-Path $RepoDir "assets\v_fs_backup_logo.ico"

if (-not (Test-Path $LogoPng)) { throw "Missing logo asset at $LogoPng" }
if (-not (Test-Path $IconIco)) { throw "Missing Windows icon asset at $IconIco" }

$CodeSigningCertificate = Resolve-CodeSigningCertificate
Ensure-RustTarget

Push-Location $RepoDir
try {
    if (-not $NoUpdate) {
        cargo update
        if ($LASTEXITCODE -ne 0) { throw "cargo update failed with exit code $LASTEXITCODE" }
    }
    $cargoArgs = @("build", "--release", "--target", $RustTarget)
    if ($Locked) { $cargoArgs += "--locked" }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}

if (-not (Test-Path $AppExe)) { throw "Release binary not found at $AppExe" }

Invoke-CodeSignFile -Certificate $CodeSigningCertificate -FilePath $AppExe -Description "$CliBin release binary"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem -Path $OutDir -Filter "$ArtifactBaseName*" -File -ErrorAction SilentlyContinue | Remove-Item -Force

New-StageDirectory -StageDir $StageDir -AppExe $AppExe -LogoPng $LogoPng -IconIco $IconIco
Compress-Archive -Path (Join-Path $StageDir $AppName) -DestinationPath $OutputZip -Force
Write-Host "packaged $OutputZip"

$ArtifactNames = @("$ArtifactBaseName.zip")
$MsiCreated = $false
$ExeCreated = $false
if (Ensure-Wix) {
    $MsiCreated = Build-Msi -OutDir $OutDir -StageDir $StageDir -ArtifactBaseName $ArtifactBaseName -Version $Version -IconPath $IconIco
    if ($MsiCreated) {
        $msiPath = Join-Path $OutDir "$ArtifactBaseName.msi"
        Invoke-CodeSignFile -Certificate $CodeSigningCertificate -FilePath $msiPath -Description "$ArtifactBaseName.msi"
        $ArtifactNames += "$ArtifactBaseName.msi"
        $wixExtensionRef = Ensure-WixBootstrapperExtension
        if ($wixExtensionRef) {
            $ExeCreated = Build-ExeInstaller -OutDir $OutDir -ArtifactBaseName $ArtifactBaseName -Version $Version -WixExtensionRef $wixExtensionRef -IconPath $IconIco
            if ($ExeCreated) {
                $exePath = Join-Path $OutDir "$ArtifactBaseName.exe"
                Invoke-CodeSignBundle -Certificate $CodeSigningCertificate -BundlePath $exePath -Description "$ArtifactBaseName.exe"
                $ArtifactNames += "$ArtifactBaseName.exe"
            }
        } else {
            Write-Warning "Skipped $ArtifactBaseName.exe because the WiX bootstrapper extension is unavailable."
        }
    }
} else {
    Write-Warning "Skipped $ArtifactBaseName.msi and $ArtifactBaseName.exe."
}

Write-Checksums -OutDir $OutDir -ArtifactBaseName $ArtifactBaseName -ArtifactNames $ArtifactNames
Remove-Item $StageDir -Recurse -Force -ErrorAction SilentlyContinue

if ($NeedsReboot) {
    Write-Warning "A Windows reboot is required before every terminal can see updated build tools."
}
if ($UsingGeneratedCodeSigningCertificate) {
    $safePublisher = $ExpectedSignaturePublisher -replace '[^A-Za-z0-9._-]', '_'
    Write-Warning "These artifacts were signed with certs\$AppName-$safePublisher-local-codesign.cer. Install that certificate on target machines to trust the local signature."
}

Write-Host "Windows artifacts created under $OutDir" -ForegroundColor Green
Write-Host ""
