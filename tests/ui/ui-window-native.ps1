param([Parameter(Mandatory=$true)][int]$CockpitPid,[Parameter(Mandatory=$true)][string]$Executable,[Parameter(Mandatory=$true)][ValidateSet('inspect','resize','close','minimize','show')][string]$Action,[long]$WindowHandle=0)
$ErrorActionPreference='Stop'
$uiProcess=Get-Process -Id $CockpitPid
$uiExecutable=(Resolve-Path -LiteralPath $Executable).Path
if($uiProcess.Path -ne $uiExecutable){throw 'The selected process is not the isolated acceptance Cockpit.'}
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class UiAcceptanceWindow {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int hgt,uint flags);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int action);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
}
'@
$null=[UiAcceptanceWindow]::SetThreadDpiAwarenessContext([IntPtr](-4))
$uiHandle=if($WindowHandle -ne 0){[IntPtr]$WindowHandle}else{$uiProcess.MainWindowHandle}
if($uiHandle -eq [IntPtr]::Zero){throw 'The selected Cockpit has no main window.'}
$uiWindowPid=0
$null=[UiAcceptanceWindow]::GetWindowThreadProcessId($uiHandle,[ref]$uiWindowPid)
if($uiWindowPid -ne $CockpitPid){throw 'The selected window no longer belongs to the isolated Cockpit.'}
switch($Action){
 'resize' {$null=[UiAcceptanceWindow]::ShowWindow($uiHandle,9);if(-not[UiAcceptanceWindow]::SetWindowPos($uiHandle,[IntPtr]::Zero,80,70,1300,900,0x0014)){throw 'Could not resize the acceptance window.'}}
 'close' {$null=[UiAcceptanceWindow]::PostMessage($uiHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)}
 'minimize' {$null=[UiAcceptanceWindow]::ShowWindow($uiHandle,6)}
 'show' {Start-Process -FilePath $uiExecutable -WorkingDirectory (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $uiExecutable)))) -WindowStyle Hidden | Out-Null}
}
$uiRect=New-Object UiAcceptanceWindow+Rect
$null=[UiAcceptanceWindow]::GetWindowRect($uiHandle,[ref]$uiRect)
@{pid=$CockpitPid;handle=$uiHandle.ToInt64().ToString();visible=[UiAcceptanceWindow]::IsWindowVisible($uiHandle);x=$uiRect.Left;y=$uiRect.Top;width=$uiRect.Right-$uiRect.Left;height=$uiRect.Bottom-$uiRect.Top}|ConvertTo-Json -Compress
