# A reversible report roundtrip on an existing FREE slot; never provisions or resets.
param([uint32]$Slot = 1)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VJoyRoundtrip {
  const string Dll = @"C:\Program Files\vJoy\x64\vJoyInterface.dll";
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int DriverMatch(ref ushort dll, ref ushort driver);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDStatus(uint id);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int AcquireVJD(uint id);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern void RelinquishVJD(uint id);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern uint GetPosition(uint id, IntPtr data);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int UpdateVJD(uint id, IntPtr data);
  [StructLayout(LayoutKind.Sequential, Pack=4)]
  public struct PositionV3 {
    public byte device;
    [MarshalAs(UnmanagedType.ByValArray, SizeConst=30)] public int[] words;
  }
}
'@
[ushort]$dll = 0
[ushort]$driver = 0
if ([VJoyRoundtrip]::DriverMatch([ref]$dll, [ref]$driver) -ne 1 -or $dll -ne 0x222 -or $driver -ne 0x222) {
    throw 'This probe requires the selected 2.2.2 driver and matching DLL.'
}
$reportSize = [Runtime.InteropServices.Marshal]::SizeOf([type][VJoyRoundtrip+PositionV3])
if ($reportSize -ne 124) { throw 'Unexpected V3 report ABI size.' }
if ([VJoyRoundtrip]::GetVJDStatus($Slot) -ne 1) { throw 'Slot is not FREE; refusing to take it over.' }
$owned = $false
$report = [Runtime.InteropServices.Marshal]::AllocHGlobal($reportSize)
$readback = [Runtime.InteropServices.Marshal]::AllocHGlobal($reportSize)
try {
    $zero = [byte[]]::new($reportSize)
    [Runtime.InteropServices.Marshal]::Copy($zero, 0, $report, $reportSize)
    [Runtime.InteropServices.Marshal]::Copy($zero, 0, $readback, $reportSize)
    if ([VJoyRoundtrip]::AcquireVJD($Slot) -ne 1) { throw 'Acquisition failed.' }
    $owned = $true
    if ([VJoyRoundtrip]::GetPosition($Slot, $report) -ne 0) { throw 'Could not capture the current report.' }
    # Re-submit the captured full report without changing any game-visible controls.
    if ([VJoyRoundtrip]::UpdateVJD($Slot, $report) -ne 1) { throw 'Full report submission failed.' }
    if ([VJoyRoundtrip]::GetPosition($Slot, $readback) -ne 0) { throw 'Report readback failed.' }
    $expected = [byte[]]::new($reportSize)
    $actual = [byte[]]::new($reportSize)
    [Runtime.InteropServices.Marshal]::Copy($report, $expected, 0, $reportSize)
    [Runtime.InteropServices.Marshal]::Copy($readback, $actual, 0, $reportSize)
    # Ignore structure padding following bDevice; compare every declared report field.
    if ($expected[0] -ne $actual[0] -or
        [Convert]::ToBase64String($expected, 4, 120) -cne [Convert]::ToBase64String($actual, 4, 120)) {
        throw 'Driver readback does not match the submitted report.'
    }
    [ordered]@{ slot=$Slot; reportBytes=$reportSize; driverVersion=$driver; roundtrip='passed' } |
        ConvertTo-Json
} finally {
    if ($owned) { [VJoyRoundtrip]::RelinquishVJD($Slot) }
    [Runtime.InteropServices.Marshal]::FreeHGlobal($report)
    [Runtime.InteropServices.Marshal]::FreeHGlobal($readback)
}
