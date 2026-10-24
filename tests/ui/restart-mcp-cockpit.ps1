param(
  [Parameter(Mandatory=$true)][string]$Executable,
  [Parameter(Mandatory=$true)][string]$Workspace,
  [Parameter(Mandatory=$true)][string]$ConfigPath,
  [Parameter(Mandatory=$true)][int]$UiPort,
  [Parameter(Mandatory=$true)][int]$DebugPort
)
$ErrorActionPreference = 'Stop'
$mcpExecutable = (Resolve-Path -LiteralPath $Executable).Path
$mcpWorkspace = (Resolve-Path -LiteralPath $Workspace).Path
New-Item -ItemType Directory -Path (Join-Path $mcpWorkspace '.research/dev') -Force | Out-Null
# Stop only this exact native binary; other worktree Cockpits use other paths.
$mcpProcesses = Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $mcpExecutable }
foreach ($mcpProcess in $mcpProcesses) { Stop-Process -Id $mcpProcess.ProcessId -ErrorAction SilentlyContinue }
Start-Sleep -Milliseconds 2000
$mcpNode = (Get-Command node).Source
$mcpUiReady = $false
try { $mcpUiReady = (Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$UiPort/" -TimeoutSec 2).StatusCode -eq 200 } catch {}
if (-not $mcpUiReady) {
  $mcpVitePath = Join-Path $mcpWorkspace 'node_modules/vite/bin/vite.js'
  Start-Process -FilePath $mcpNode -ArgumentList @($mcpVitePath,'--host','127.0.0.1','--port',"$UiPort",'--strictPort') -WorkingDirectory $mcpWorkspace -WindowStyle Hidden -RedirectStandardOutput (Join-Path $mcpWorkspace '.research/dev/restart-vite.log') -RedirectStandardError (Join-Path $mcpWorkspace '.research/dev/restart-vite-error.log') | Out-Null
}
$env:OSG_DCS_BIOS_CONFIG = $ConfigPath
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
$mcpStarted = Start-Process -FilePath $mcpExecutable -WorkingDirectory $mcpWorkspace -WindowStyle Hidden -RedirectStandardOutput (Join-Path $mcpWorkspace '.research/dev/restart-app.log') -RedirectStandardError (Join-Path $mcpWorkspace '.research/dev/restart-app-error.log') -PassThru
@{pid=$mcpStarted.Id;uiPort=$UiPort;debugPort=$DebugPort} | ConvertTo-Json -Compress
