<# Stage the Windows executable containing the private mapping workers. Never install drivers or change qualification. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateSet('development', 'release')][string]$BuildKind = 'development'
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Run staging on native Windows.' }
$repository = Split-Path -Parent $PSScriptRoot
$noticeNames = @('LICENSE', 'THIRD_PARTY_NOTICES.md')
$dependencyRoot = Join-Path $repository 'src-tauri/resources/dependencies'
$dependencies = @(Get-Content -LiteralPath (Join-Path $dependencyRoot 'manifest.json') -Raw | ConvertFrom-Json)
foreach ($dependency in $dependencies) {
    $installer = Join-Path $dependencyRoot $dependency.filename
    if ((Get-Item -LiteralPath $installer).Length -ne $dependency.size -or
        (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne $dependency.sha256) {
        throw "Optional installer does not match the pinned catalog: $($dependency.id)"
    }
    if ((Get-AuthenticodeSignature -LiteralPath $installer).Status -ne 'Valid') {
        throw "Optional installer has no valid publisher signature: $($dependency.id)"
    }
}
foreach ($name in $noticeNames) {
    if (-not (Test-Path -LiteralPath (Join-Path $repository $name) -PathType Leaf)) {
        throw "Missing distribution notice: $name"
    }
}
$source = Get-Item -LiteralPath $Executable
if ($source.Name -ne 'osg-cockpit.exe' -or $source.PSIsContainer) {
    throw 'Select the built osg-cockpit.exe desktop artifact.'
}
if ($source.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Select a regular build artifact.' }
$signature = Get-AuthenticodeSignature -LiteralPath $source.FullName
if ($BuildKind -eq 'release' -and $signature.Status -ne 'Valid') {
    throw 'Release staging requires a valid Authenticode signature from the release signing process.'
}
$digest = (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
$output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
New-Item -ItemType Directory -Path $output -Force | Out-Null
$destination = Join-Path $output 'osg-cockpit.exe'
$manifest = Join-Path $output 'qualification.json'
$dependencyDestination = Join-Path $output 'dependencies'
if (Test-Path -LiteralPath $dependencyDestination) {
    throw 'Use a new staging directory; existing dependencies are not replaced.'
}
if ((Test-Path -LiteralPath $destination) -or (Test-Path -LiteralPath $manifest)) {
    throw 'Use a new staging directory; existing artifacts are not replaced.'
}
foreach ($name in $noticeNames) {
    if (Test-Path -LiteralPath (Join-Path $output $name)) {
        throw 'Use a new staging directory; existing notices are not replaced.'
    }
}
Copy-Item -LiteralPath $source.FullName -Destination $destination
$stagedDigest = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
if ($stagedDigest -ne $digest) { throw 'Build artifact changed during staging.' }
$stagedSignature = Get-AuthenticodeSignature -LiteralPath $destination
if ($stagedSignature.Status -ne $signature.Status) { throw 'Signature status changed during staging.' }
foreach ($name in $noticeNames) {
    Copy-Item -LiteralPath (Join-Path $repository $name) -Destination (Join-Path $output $name)
}
Copy-Item -LiteralPath $dependencyRoot -Destination $dependencyDestination -Recurse
foreach ($dependency in $dependencies) {
    $installer = Join-Path $dependencyDestination $dependency.filename
    if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne $dependency.sha256) {
        throw "Optional installer changed during staging: $($dependency.id)"
    }
}
$evidence = [ordered]@{
    schema = 1
    qualified = $false
    buildKind = $BuildKind
    artifact = 'osg-cockpit.exe'
    sha256 = $stagedDigest
    signatureStatus = $stagedSignature.Status.ToString()
    signer = if ($stagedSignature.SignerCertificate) { $stagedSignature.SignerCertificate.Subject } else { $null }
    os = [Environment]::OSVersion.VersionString
    helperPackaging = 'Private modes of the same executable; no replacement helper path from settings.'
    optionalDependenciesBundled = @($dependencies | Select-Object id, version, filename, sha256)
    gates = 'Live output, provisioning, visibility and live script output remain disabled.'
    remaining = 'Signed installer/driver setup, UAC/upgrade/recovery, physical routing and real-game qualification.'
}
$evidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifest -Encoding utf8
$evidence | ConvertTo-Json -Depth 5
