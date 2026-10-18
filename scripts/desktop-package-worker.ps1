<# Stage the development executable and probe private-worker startup with empty stdin. Never sends input commands or installs dependencies. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Run this probe on native Windows.' }
$repository = Split-Path -Parent $PSScriptRoot
$research = [IO.Path]::GetFullPath((Join-Path $repository '.research')) + [IO.Path]::DirectorySeparatorChar
$output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (-not $output.StartsWith($research, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Use a new output directory below this repository''s .research directory.'
}
if (Test-Path -LiteralPath $output) { throw 'Use a new output directory; evidence is never replaced.' }
New-Item -ItemType Directory -Path $output | Out-Null
$source = Get-Item -LiteralPath $Executable
$sourceDigest = (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
$package = Join-Path $output 'package'
& (Join-Path $PSScriptRoot 'package-windows-mapping.ps1') -Executable $source.FullName -OutputDirectory $package -BuildKind development | Out-Null
$staged = Join-Path $package 'osg-cockpit.exe'
$manifest = Get-Content -LiteralPath (Join-Path $package 'qualification.json') -Raw | ConvertFrom-Json
if ($manifest.qualified -ne $false -or $manifest.buildKind -ne 'development' -or $manifest.sha256 -ne $sourceDigest) {
    throw 'Staging did not preserve the development artifact and closed qualification.'
}
$stdin = Join-Path $output 'empty-stdin'
[IO.File]::WriteAllBytes($stdin, [byte[]]@())
$checks = @()
for ($cycle = 1; $cycle -le 3; $cycle++) {
    $stdout = Join-Path $output "worker-$cycle.stdout"
    $stderr = Join-Path $output "worker-$cycle.stderr"
    # The sole recognized private argument bypasses Tauri startup. Empty stdin
    # provides immediate EOF: no Activate, Arm, Deliver or Recover is written.
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $staged -ArgumentList '--osg-desktop-input-worker' -WorkingDirectory $package -WindowStyle Hidden -RedirectStandardInput $stdin -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    try {
        if (-not $process.WaitForExit(5000)) {
            throw 'Private worker did not exit within the five-second startup/EOF bound.'
        }
        $timer.Stop()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "Private worker exited with code $($process.ExitCode); inspect the local logs." }
        if ((Get-Item -LiteralPath $stdout).Length -gt 1024 -or (Get-Item -LiteralPath $stderr).Length -ne 0) {
            throw 'Private worker output exceeded the hello bound or stderr was not empty.'
        }
        $lines = @(Get-Content -LiteralPath $stdout)
        if ($lines.Count -ne 1) { throw 'Expected exactly one worker hello and no acknowledgements.' }
        $hello = $lines[0] | ConvertFrom-Json
        $fields = @($hello.PSObject.Properties.Name | Sort-Object)
        if (($fields -join ',') -ne 'backend,ok,protocol' -or $hello.protocol -ne 1 -or $hello.backend -ne 'desktop-input' -or $hello.ok -ne $true) {
            throw 'Private worker hello did not match the bounded desktop-input protocol.'
        }
        $checks += [ordered]@{
            cycle = $cycle
            hello = $hello
            exitCode = $process.ExitCode
            startupAndEofExitMs = $timer.ElapsedMilliseconds
            stdoutBytes = (Get-Item -LiteralPath $stdout).Length
            stderrBytes = 0
            stdinBytes = (Get-Item -LiteralPath $stdin).Length
        }
    } finally {
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force
            if (-not $process.WaitForExit(1000)) { throw 'The owned probe process did not stop.' }
        }
        $process.Dispose()
    }
}
$stagedDigest = (Get-FileHash -LiteralPath $staged -Algorithm SHA256).Hash.ToLowerInvariant()
if ($sourceDigest -ne $stagedDigest -or $sourceDigest -ne (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash.ToLowerInvariant()) {
    throw 'Source or staged executable changed during the probe.'
}
$evidence = [ordered]@{
    schema = 1
    result = 'pass'
    experiment = 'Development staged Windows executable private desktop-worker startup and EOF shutdown'
    buildKind = 'development'
    artifact = 'osg-cockpit.exe'
    sha256 = $stagedDigest
    signatureStatus = $manifest.signatureStatus
    stagingScript = 'scripts/package-windows-mapping.ps1'
    sameExecutableHashUnchanged = $true
    workingDirectory = 'staged package directory'
    soleArgument = '--osg-desktop-input-worker'
    protocolCommandsSent = 0
    stdin = 'Empty redirected file; immediate EOF'
    startupAndEofExitBoundMs = 5000
    helloMaxBytes = 1024
    checks = $checks
    sessionLockReacquiredAcrossSequentialLaunches = $true
    nativeInputSubmitted = $false
    noInputBasis = 'No protocol frames sent; the audited worker starts with no runs or held controls and empty cleanup skips native submit. Native input APIs were not instrumented by this probe.'
    productionQualified = $false
    measurementBoundary = 'Same built executable copied by the development packaging script, launched from its staged directory, emitted its private hello, and exited cleanly on EOF three times. No cockpit runtime path, input delivery, installer, upgrade, driver setup, game, hardware or physical overlap qualification.'
}
$evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'evidence.json') -Encoding utf8
$evidence | ConvertTo-Json -Depth 8
