<# Package the Windows x64 installer and a portable development build. Never run an installer. #>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Run Windows release packaging on native Windows.' }
$repository = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $repository 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$') { throw 'Invalid application version.' }
$packageVersion = (Get-Content -LiteralPath (Join-Path $repository 'package.json') -Raw | ConvertFrom-Json).version
if ($version -ne $packageVersion) { throw 'Frontend and native application versions differ.' }
if ($env:GITHUB_REF -like 'refs/tags/cockpit-v*' -and $env:GITHUB_REF_NAME -ne "cockpit-v$version") {
    throw 'The version tag must match the application version.'
}
$build = Join-Path $repository 'src-tauri/target/release'
$installers = @(Get-ChildItem -LiteralPath (Join-Path $build 'bundle/nsis') -File -Filter '*-setup.exe')
if ($installers.Count -ne 1) { throw 'Expected exactly one built NSIS installer.' }
$installer = $installers[0]
if ($installer.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Expected a regular installer file.' }
$output = Join-Path $repository '.research/windows-release'
# Refuse to mix an old package with a new build; no recursive cleanup is performed.
if (Test-Path -LiteralPath $output) { throw 'Use a clean Windows release staging directory.' }
$portable = Join-Path $output 'portable/OpenSimGear-Cockpit'
$artifacts = Join-Path $output 'artifacts'
New-Item -ItemType Directory -Path $artifacts -Force | Out-Null
& (Join-Path $PSScriptRoot 'package-windows-mapping.ps1') -Executable (Join-Path $build 'osg-cockpit.exe') -OutputDirectory $portable -BuildKind development
Copy-Item -LiteralPath (Join-Path $repository 'docs/windows-release-notes.md') -Destination (Join-Path $portable 'README.md')
$prefix = "OpenSimGear-Cockpit_${version}_windows_x64"
Copy-Item -LiteralPath $installer.FullName -Destination (Join-Path $artifacts "${prefix}-setup.exe")
Compress-Archive -Path $portable -DestinationPath (Join-Path $artifacts "${prefix}-portable.zip") -CompressionLevel Optimal
$files = @(Get-ChildItem -LiteralPath $artifacts -File | Sort-Object Name)
$checksums = foreach ($file in $files) {
    $digest = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$digest  $($file.Name)"
}
$checksums | Set-Content -LiteralPath (Join-Path $artifacts 'SHA256SUMS') -Encoding ascii
Get-ChildItem -LiteralPath $artifacts -File | Select-Object Name, Length
