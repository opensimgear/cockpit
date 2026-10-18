# M0 experiment only. Adds a temporary slot on the selected 2.2.2 default layout.
# No vJoyConfig invocation: that tool can delete global OEM force-feedback entries.
param(
    [ValidateSet('Plan','Run','Fail','Watch')][string]$Mode = 'Plan',
    [Parameter(Mandatory)][string]$Directory,
    [int]$ParentId = 0,
    [long]$ParentStartTicks = 0
)
$ErrorActionPreference = 'Stop'
$Directory = (Resolve-Path -LiteralPath $Directory).ProviderPath
$script:Parameters = 'SYSTEM\CurrentControlSet\Services\vjoy\Parameters'
$script:ExpectedDescriptorHash = '3CCC044EC270E74CB588C56B1B49C7E9ABA74EB878666EFCA43855D6B1EA1E93'
$script:Owned = @('Device01','Device02')
$script:Shell = (Get-Process -Id $PID).Path
$script:ActivePath = Join-Path $Directory 'provision-active.json'
$script:Baseline = $null

function Save-New([string]$Path, $Value) {
    $bytes = [Text.Encoding]::UTF8.GetBytes(($Value | ConvertTo-Json -Depth 20 -Compress))
    $file = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $file.Write($bytes,0,$bytes.Length); $file.Flush($true) } finally { $file.Dispose() }
}
function Assert-Admin {
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Provisioning requires an elevated helper; no settings changed.'
    }
}
function Read-Inventory {
    $text = & $script:Shell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Directory 'probe-vjoy.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'Fresh inventory process failed.' }
    return (($text -join [Environment]::NewLine) | ConvertFrom-Json)
}
function Assert-Slots($Inventory, [int[]]$Ids) {
    if (!$Inventory.driverMatches -or $Inventory.driverVersion -ne 546 -or $Inventory.dllVersion -ne 546) { throw 'Selected driver/DLL mismatch.' }
    if (($Inventory.slots.id -join ',') -ne ($Ids -join ',')) { throw 'Unexpected published slot set.' }
    $usages = @(48,49,50,51,52,53,54,55,56,196,197,198,200,176,186,187)
    foreach ($slot in $Inventory.slots) {
        if ($slot.status -ne 1) { throw 'A slot is not FREE; refusing reconfiguration.' }
        if ($slot.buttons -ne 8 -or $slot.discretePovs -ne 0 -or $slot.continuousPovs -ne 0 -or $slot.forceFeedback) { throw 'Unsupported existing layout.' }
        if (($slot.axes.usage -join ',') -ne ($usages -join ',')) { throw 'Unexpected axes.' }
        if (@($slot.axes | Where-Object { $_.minimum -ne 0 -or $_.maximum -ne 32767 }).Count) { throw 'Unexpected axis range.' }
    }
}
function Registry-Tree($Key, [int]$Depth = 0) {
    if ($Depth -gt 8) { throw 'OEM tree exceeds bound.' }
    $values = [ordered]@{}
    foreach ($name in @($Key.GetValueNames() | Sort-Object)) {
        $values[$name] = @($Key.GetValueKind($name).ToString(), $Key.GetValue($name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames))
    }
    $children = [ordered]@{}
    foreach ($name in @($Key.GetSubKeyNames() | Sort-Object)) {
        $child = $Key.OpenSubKey($name)
        try { $children[$name] = Registry-Tree $child ($Depth+1) } finally { $child.Dispose() }
    }
    return [ordered]@{ values=$values; children=$children }
}
function Oem-Fingerprint {
    $snapshot = [ordered]@{}
    foreach ($hiveName in @('LocalMachine','CurrentUser')) {
        $hive = [Microsoft.Win32.Registry]::$hiveName
        foreach ($device in @('VID_1234&PID_BEAD','VID_1234&PID_0FFB')) {
            $key = $hive.OpenSubKey('SYSTEM\CurrentControlSet\Control\MediaProperties\PrivateProperties\Joystick\OEM\' + $device)
            try { $snapshot[$hiveName + '\' + $device] = $(if ($key) { Registry-Tree $key } else { $null }) }
            finally { if ($key) { $key.Dispose() } }
        }
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($snapshot | ConvertTo-Json -Depth 30 -Compress))
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [Convert]::ToBase64String($sha.ComputeHash($bytes)) } finally { $sha.Dispose() }
}
function Descriptor([int]$Slot) {
    $bytes = [IO.File]::ReadAllBytes((Join-Path $Directory 'default-descriptor.bin'))
    if ($bytes.Length -ne 123 -or $bytes[10] -ne 0x85 -or $bytes[11] -ne 1) { throw 'Unexpected selected-release descriptor.' }
    $bytes[11] = [byte]$Slot
    return ,$bytes
}
function Check-Registry([switch]$Initial) {
    $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($script:Parameters)
    if (!$key) { throw 'Missing vJoy parameter key.' }
    try {
        if ($key.GetValueNames().Length -ne 0) { throw 'Unexpected parameter values; preserve existing configuration.' }
        foreach ($name in $key.GetSubKeyNames()) {
            if ($name -eq 'Wdf') { continue }
            if ($Initial -or $name -notin $script:Owned) { throw 'Existing or externally added descriptor; refusing changes.' }
            $child = $key.OpenSubKey($name)
            try {
                if ($child.SubKeyCount -ne 0) { throw 'Unexpected child under owned descriptor.' }
                $names = @($child.GetValueNames())
                # A crash can occur between key creation and either of the two value writes.
                if (@($names | Where-Object { $_ -notin @('HidReportDescriptor','HidReportDescriptorSize') }).Count) { throw 'External descriptor values; preserving conflict.' }
                $expected = Descriptor ([int]$name.Substring(6))
                if ('HidReportDescriptor' -in $names) {
                    if ($child.GetValueKind('HidReportDescriptor') -ne [Microsoft.Win32.RegistryValueKind]::Binary -or
                        [Convert]::ToBase64String($child.GetValue('HidReportDescriptor')) -cne [Convert]::ToBase64String($expected)) { throw 'External descriptor edit; preserving conflict.' }
                }
                if ('HidReportDescriptorSize' -in $names) {
                    if ($child.GetValueKind('HidReportDescriptorSize') -ne [Microsoft.Win32.RegistryValueKind]::DWord -or
                        $child.GetValue('HidReportDescriptorSize') -ne $expected.Length) { throw 'External size edit; preserving conflict.' }
                }
            } finally { $child.Dispose() }
        }
    } finally { $key.Dispose() }
}
function Restart-VJoy {
    $out = & pnputil.exe /restart-device 'ROOT\HIDCLASS\0000' 2>&1
    if ($LASTEXITCODE -ne 0) { throw ('vJoy restart failed or requires reboot: ' + ($out -join ' ')) }
    Add-Content -LiteralPath (Join-Path $Directory 'restart.log') -Value $out
}
function Restore-Owned {
    Assert-Admin
    Check-Registry
    # A different feeder may have acquired a published slot after a failure.
    $inventory = Read-Inventory
    if (@($inventory.slots | Where-Object { $_.status -ne 1 }).Count) { throw 'Recovery found a non-FREE slot; preserve journal and do not interrupt its owner.' }
    $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($script:Parameters,$true)
    try {
        foreach ($name in @('Device02','Device01')) {
            if ($name -in $key.GetSubKeyNames()) { $key.DeleteSubKey($name,$false) }
        }
    } finally { $key.Dispose() }
    Restart-VJoy
    Assert-Slots (Read-Inventory) @(1)
    Check-Registry -Initial
    if ((Oem-Fingerprint) -cne $script:Baseline.oemFingerprint) { throw 'OEM FFB registration changed externally; retained evidence without overwriting it.' }
}
function Preflight {
    $rootDevice = Get-PnpDevice -InstanceId 'ROOT\HIDCLASS\0000'
    $rootService = (Get-PnpDeviceProperty -InstanceId $rootDevice.InstanceId -KeyName DEVPKEY_Device_Service).Data
    if ($rootDevice.Status -ne 'OK' -or $rootService -ne 'vjoy') { throw 'Expected vJoy root device is not ready; refusing to restart another device.' }
    if ((Get-FileHash -LiteralPath (Join-Path $Directory 'default-descriptor.bin') -Algorithm SHA256).Hash -ne $script:ExpectedDescriptorHash) { throw 'Default descriptor hash mismatch.' }
    if ((Get-FileHash -LiteralPath 'C:\Windows\System32\drivers\vjoy.sys' -Algorithm SHA256).Hash -ne '540AC46BC529001798068BD402BB4C36891D4C0E63B9DAC9761B14BD38667D7D') { throw 'Driver is not the qualified signed release.' }
    if (Get-Process -Name acr,acs,DCS,RacingLab,vJoyConf,vJoyConfig -ErrorAction SilentlyContinue) { throw 'Close games, feeders and vJoy configuration tools first.' }
    Check-Registry -Initial
    $inventory = Read-Inventory
    Assert-Slots $inventory @(1)
    return [ordered]@{ schema=1; slots=$inventory.slots; oemFingerprint=(Oem-Fingerprint); ownedKeys=$script:Owned; defaultDescriptorSha256=$script:ExpectedDescriptorHash }
}

if ($Mode -eq 'Watch') {
    try {
        Assert-Admin
        $script:Baseline = Get-Content -LiteralPath $script:ActivePath -Raw | ConvertFrom-Json
        $parent = Get-Process -Id $ParentId
        if ($parent.StartTime.ToUniversalTime().Ticks -ne $ParentStartTicks) { throw 'Guardian parent identity mismatch.' }
        Save-New (Join-Path $Directory 'guardian-ready.json') @{ ready=$true }
        if (!$parent.WaitForExit(120000)) { $parent.Kill(); $parent.WaitForExit(5000) | Out-Null }
        if (Test-Path -LiteralPath (Join-Path $Directory 'restored.json')) {
            Save-New (Join-Path $Directory 'guardian-result.json') @{ restored=$true; alreadyRestored=$true }
        } else {
            Restore-Owned
            Save-New (Join-Path $Directory 'guardian-result.json') @{ restored=$true; alreadyRestored=$false }
        }
    } catch {
        Save-New (Join-Path $Directory 'guardian-error.json') @{ error=$_.Exception.Message }
        exit 1
    }
    exit 0
}
$script:Baseline = Preflight
if ($Mode -eq 'Plan') {
    [ordered]@{ ready=$true; changes=@('Materialize default slot 1','Temporarily add identical slot 2','Restart vJoy only','Check both layouts and report slot 2','Remove only these two keys and restore implicit slot 1'); snapshot=$script:Baseline } | ConvertTo-Json -Depth 10
    exit 0
}
Assert-Admin
Save-New $script:ActivePath $script:Baseline
$parent = Get-Process -Id $PID
$guardArguments = @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $PSCommandPath + '"'),'-Mode','Watch','-Directory',('"' + $Directory + '"'),'-ParentId',$PID,'-ParentStartTicks',$parent.StartTime.ToUniversalTime().Ticks)
$guardian = Start-Process -FilePath $script:Shell -ArgumentList $guardArguments -WindowStyle Hidden -PassThru
$timer = [Diagnostics.Stopwatch]::StartNew()
while (!(Test-Path -LiteralPath (Join-Path $Directory 'guardian-ready.json')) -and !$guardian.HasExited -and $timer.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 20 }
if (!(Test-Path -LiteralPath (Join-Path $Directory 'guardian-ready.json')) -or $guardian.HasExited) { throw 'Guardian unavailable; no registry mutation performed.' }
try {
    Check-Registry -Initial
    $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($script:Parameters,$true)
    try {
        foreach ($slot in @(1,2)) {
            $name = 'Device{0:D2}' -f $slot
            if ($name -in $key.GetSubKeyNames()) { throw 'Descriptor appeared externally; do not overwrite.' }
            $child = $key.CreateSubKey($name)
            try {
                $bytes = Descriptor $slot
                $child.SetValue('HidReportDescriptor',$bytes,[Microsoft.Win32.RegistryValueKind]::Binary)
                $child.SetValue('HidReportDescriptorSize',$bytes.Length,[Microsoft.Win32.RegistryValueKind]::DWord)
                $child.Flush()
            } finally { $child.Dispose() }
        }
    } finally { $key.Dispose() }
    Check-Registry
    Restart-VJoy
    $during = Read-Inventory
    Assert-Slots $during @(1,2)
    if ((Oem-Fingerprint) -cne $script:Baseline.oemFingerprint) { throw 'FFB registration changed.' }
    if ($Mode -eq 'Fail') { throw 'Injected failure after provisioning.' }
    $roundtrip = & $script:Shell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Directory 'probe-vjoy-report.ps1') -Slot 2
    if ($LASTEXITCODE -ne 0) { throw 'Provisioned slot report test failed.' }
    Save-New (Join-Path $Directory 'during.json') @{ inventory=$during; report=($roundtrip -join [Environment]::NewLine | ConvertFrom-Json) }
} catch {
    Save-New (Join-Path $Directory 'experiment-error.json') @{ error=$_.Exception.Message }
    throw
} finally {
    Restore-Owned
    Save-New (Join-Path $Directory 'restored.json') @{ restored=$true; ffbRegistrationUnchanged=$true }
}
Save-New (Join-Path $Directory 'result.json') @{ passed=$true; restored=$true; temporarySlot=2; preservedSlot=1; ffbRegistrationUnchanged=$true }
