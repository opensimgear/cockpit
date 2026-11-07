param([Parameter(Mandatory=$true)][uint32]$Slot)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VJoyExternalOwner {
  const string Dll = @"C:\Program Files\vJoy\x64\vJoyInterface.dll";
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int DriverMatch(ref ushort dll, ref ushort driver);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int GetVJDStatus(uint id);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern int AcquireVJD(uint id);
  [DllImport(Dll, CallingConvention=CallingConvention.Cdecl)] public static extern void RelinquishVJD(uint id);
}
'@
[uint16]$dll = 0; [uint16]$driver = 0
if ([VJoyExternalOwner]::DriverMatch([ref]$dll, [ref]$driver) -ne 1 -or $dll -ne 0x222 -or $driver -ne 0x222) {
  throw 'This fixture requires the selected 2.2.2 driver and matching DLL.'
}
if ([VJoyExternalOwner]::GetVJDStatus($Slot) -ne 1) { throw 'Slot is not FREE; refusing to take it over.' }
if ([VJoyExternalOwner]::AcquireVJD($Slot) -ne 1) { throw 'Acquisition failed.' }
try {
  [ordered]@{state='ready';pid=$PID;slot=$Slot} | ConvertTo-Json -Compress
  $line = [Console]::In.ReadLine()
  if ($line -ne 'release') { throw 'Unexpected feeder command.' }
} finally {
  [VJoyExternalOwner]::RelinquishVJD($Slot)
}
@{state='released';slot=$Slot} | ConvertTo-Json -Compress
