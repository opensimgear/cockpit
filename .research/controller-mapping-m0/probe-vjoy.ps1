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
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDAxisMin(uint id, uint axis, ref int value);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDAxisMax(uint id, uint axis, ref int value);
[DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int IsDeviceFfb(uint id);
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
      forceFeedback = ([VJoyProbe]::IsDeviceFfb($slot) -eq 1)
      # 2.2.2 also exposes seven simulation usages outside the 0x30..0x38 range.
      axes = @(@(0x30,0x31,0x32,0x33,0x34,0x35,0x36,0x37,0x38,0xC4,0xC5,0xC6,0xC8,0xB0,0xBA,0xBB) | ForEach-Object {
        $axis = $_
        $exists = [VJoyProbe]::GetVJDAxisExist($slot, $axis)
        if ($exists -eq 1) {
          [int]$minimum = 0
          [int]$maximum = 0
          if ([VJoyProbe]::GetVJDAxisMin($slot, $axis, [ref]$minimum) -ne 1 -or
              [VJoyProbe]::GetVJDAxisMax($slot, $axis, [ref]$maximum) -ne 1) { throw 'Axis range query failed.' }
          [ordered]@{ usage=$axis; minimum=$minimum; maximum=$maximum }
        }
      })
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
