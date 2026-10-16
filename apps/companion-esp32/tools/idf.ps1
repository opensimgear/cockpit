# Run this checkout's ESP-IDF command in its WSL distribution.
# Arguments are forwarded as separate values; no shell command is constructed.
$ErrorActionPreference = 'Stop'
$scriptPath = Join-Path $PSScriptRoot 'idf.sh'
if ($scriptPath -match '^\\\\(?:wsl\.localhost|wsl\$)\\([^\\]+)\\(.*)$') {
    $distribution = $Matches[1]
    $linuxScript = '/' + ($Matches[2] -replace '\\', '/')
} else {
    $distribution = 'Ubuntu'
    $linuxScript = & wsl.exe -d $distribution --exec wslpath -u $scriptPath
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
& wsl.exe -d $distribution --exec bash $linuxScript @args
exit $LASTEXITCODE
