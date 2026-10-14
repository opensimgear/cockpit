param([string]$OutputPath)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VJoyProbe {
private const string Dll = @"C:\Program Files\vJoy\x64\vJoyInterface.dll";
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern short GetvJoyVersion();
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern bool vJoyEnabled();
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern bool DriverMatch(ref ushort dll, ref ushort driver);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDStatus(uint id);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetOwnerPid(uint id);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDButtonNumber(uint id);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDDiscPovNumber(uint id);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDContPovNumber(uint id);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDAxisExist(uint id, uint axis);
}
'@
[ushort]$dllVersion = 0
[ushort]$driverVersion = 0
$driverMatches = [VJoyProbe]::DriverMatch([ref]$dllVersion, [ref]$driverVersion)
$slots = @(1..16 | ForEach-Object {
  $slot = $_
  $status = [VJoyProbe]::GetVJDStatus($slot)
  if ($status -ne 3) {
    [ordered]@{
      id = $slot
      status = $status
      ownerPid = [VJoyProbe]::GetOwnerPid($slot)
      buttons = [VJoyProbe]::GetVJDButtonNumber($slot)
      discretePovs = [VJoyProbe]::GetVJDDiscPovNumber($slot)
      continuousPovs = [VJoyProbe]::GetVJDContPovNumber($slot)
      axes = @(0x30..0x39 | Where-Object { [VJoyProbe]::GetVJDAxisExist($slot, $_) -eq 1 })
    }
  }
})
$result = [ordered]@{
  enabled = [VJoyProbe]::vJoyEnabled()
  sdkVersion = [VJoyProbe]::GetvJoyVersion()
  dllVersion = $dllVersion
  driverVersion = $driverVersion
  driverMatches = $driverMatches
  slots = $slots
} | ConvertTo-Json -Depth 6
if ($OutputPath) { [IO.File]::WriteAllText($OutputPath, $result) }
$result
