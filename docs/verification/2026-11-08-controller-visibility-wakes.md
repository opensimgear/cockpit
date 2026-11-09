# Windows controller visibility waits

The HidHide worker registers a report watch for the selected devices before
it mutates policy. After hiding, it records the current baseline and waits
for a strictly newer report from each selected controller. Report commits
and cancellation wake the worker directly. An unchanged control value can
still prove a new report. The controller session, generation and interface
must match, and the two-second phase deadline still bounds the wait.

After the fresh baseline, the report watch retires. Registry inventory
commits wake the active worker for source validation. Queued handoff reads
and Stop also wake it directly. The first revision kept a separate 250 ms
external HidHide policy, executable and device-instance audit.

## Local verification

All 170 Windows controller tests and 37 configured production line coverage
checks passed. The visibility worker reached 91.50%, registry inventory
97.03%, capture registration 100%, and the retained handoff reader 92.71%.
The regressions cover a report between reading and waiting, a repeated
value with a newer sequence, source loss after arming, watch contention,
freshness timeout, and cancellation with journal restoration.

The rebuilt native dev Cockpit passed the controller catalogue and delayed
preview tests against an attached VKB Gladiator. Two catalogue readers saw
the saved rename, and the reopened reader showed the restored name. A slow
preview held one delivery while native capture advanced from sequence 43
to 53, then resumed at 53 with zero errors.

Executable SHA-256
`fb69d5251ea21906baf9b8728f9c67368badf31068f7a2d0b1e9d0128ab44787`.
The live tests used a private Cockpit database and passive input. The driver
mutation and restoration scenarios used a fake driver and the real registry
in the Windows tests. Physical HidHide apply, release and latency remain
separate acceptance in #199.

## External driver boundary

The follow-up validates the exact HID interface's PnP instance before
publishing Active and after inventory commits. The attached instance ID is
assigned to the PnP node, and Windows reports Raw Input arrival and removal
through the capture worker's device notifications. The timer now checks
HidHide policy and executable paths only. [Device instance IDs](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/device-instance-ids)
and [Raw Input device notifications](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-input-device-change).

The installed HidHide binary matched Cockpit's pinned 1.5.230 hash. A
read-only probe under a non-administrator token opened the control device and
successfully read its active flag. A second open failed with Windows error 5,
and an open after release succeeded. Opening the Parameters key with
notification rights returned access denied. The probe issued zero policy
writes. The driver marks its control device exclusive in the pinned source.
The resulting evidence applies to the installed driver and this Windows
token. [Pinned control-device source](https://github.com/nefarius/HidHide/blob/v1.5.230.0/HidHide/src/ControlDevice.c#L32-L38).

The native regression used the production registry observer and a fixture
driver. It verified two initial instance lookups, zero further lookups across
an idle audit deadline, then one lookup and owned policy restoration after a
registry commit with a replaced instance. All 171 Windows controller tests
and 37 coverage checks passed. Worker line coverage was 91.91%.

The follow-up native dev build passed the two-reader controller catalogue
test and the delayed-preview test again. The held preview resumed at
sequence 39 after capture advanced from 29, with zero errors. Its executable
SHA-256 was `263df3892d91700537e6a3ba3ef47620c4fcf6ddbd4262ce2f925dbb43cfdba7`.

## Registry notifications

The Windows account could subscribe to the HidHide service subtree even
though direct notification and value-read access to its Parameters key was
denied. The visibility worker now registers there before reading the policy.
The notification owner rearms before waking the worker, which reads the
policy through the exclusive driver handle. A missing or failed watch keeps
the existing 250 ms readback, and watch loss wakes an immediate read before
returning to that cadence. Executable path auditing still uses the 250 ms
deadline. [Windows registry notification contract](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regnotifychangekeyvalue)

The registry signal covers ordinary key and value changes. Windows excludes
`RegRestoreKey` from this notification API. Driver readback before mutation,
handoff and restoration still compares the authoritative policy. The
existing directory watcher also needs a separate executable-path design
because Windows can report an overflow with zero bytes. [Windows directory notification contract](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw)

The Windows controller fixture exercised two real subtree notifications
through a temporary current-user registry key. During a quiet executable
audit it observed zero policy reads. Deleting the subtree triggered an
immediate read and restored the existing readback cadence. Stop restored
the fixture driver's policy and left a clean journal.

The first unrestricted native run also reached the installed HidHide driver.
It reported a 65,654-byte application list and rejected bounded reads at
that size and above. The device list returned its normal two-byte empty
response. Cockpit now reports the exact oversized application-list error
while preserving the driver's existing policy. The full suite rerun passed
172 tests, including the installed driver's bounded refusal. Physical
HidHide apply and restore require a readable driver policy and remain in
#199.

All 38 controller coverage checks passed. The registry watch reached
90.91% production line coverage and the worker reached 92.92%. The rebuilt
Cockpit passed both passive controller checks and reported the exact
HidHide refusal through the native visibility status with
`changed: false`. Executable SHA-256
`1844ebf356b0c61b018b276dda450d744089703543353ea5ece5b839653d23be`.

## Executable directory watch experiment

A one-shot Windows directory notification set watched the ordinary
executable path's ancestor directories. The native fixture observed a file
change after a quiet audit period, but an ordinary rename of the selected
executable's ancestor failed with Windows error 5 while the notification
handles were open.

A separate read-only-handle probe isolated the behavior. A directory
opened with `FILE_SHARE_DELETE` still prevented an ordinary rename of its
ancestor when the handle targeted the nested `bin` directory. The rename
succeeded when the only open handle targeted the renamed directory itself
or its parent. This happened without a pending notification read.

The prototype was retired. A descendant handle can change whether the
operation being monitored succeeds. The released worker keeps its
250 ms exact executable-path audit. An alternative needs to cover
ancestor and alias changes without holding descendant directory handles,
and must surface notification loss. Windows' directory APIs distinguish
changes within a watched directory from changes to that directory itself.
[Windows directory change notifications](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstchangenotificationw)

### Volume root and drive aliases

An ordinary Windows account opened a recursive notification at the C: volume
root. The ancestor rename succeeded with the watch open. The watcher reported
both names for the ancestor and executable renames and reported a directory
junction removal and recreation. During a ten-second quiet window it delivered
113 volume records with no errors. This run used the Windows filesystem watcher
with name, attribute and security notifications. The record count is specific
to this machine and window.

A second probe created an unused drive alias to a private fixture directory.
A positive-control file creation proved the volume watcher was active.
Retargeting the same alias to another directory changed the executable's
handle-resolved NT path from `first\fixture.exe` to `second\fixture.exe`.
The watcher reported zero fixture events and zero errors during the two-second
observation. Windows allows suppressing the drive mapping broadcast, which the
probe did. The fixture removed its mapping and temporary directories.
[DefineDosDeviceW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-definedosdevicew)

A volume-root watcher can observe ordinary filesystem changes without the
descendant handle's rename interference. It cannot serve as complete continuity
proof for executable paths that pass through remappable drive aliases. The
current active-session audit remains the fallback pending an explicit decision
in #189. The probes made no HidHide driver writes.

The controller fixture uses the real Windows executable resolver with a
private executable path and a fixture driver. After the worker reaches
Active, renaming the executable's ancestor succeeds without a device
inventory change. The audit detects the missing path, restores the exact
original policy and leaves a clean journal. All 174 Windows controller
tests and 38 configured coverage checks passed. The visibility worker
reached 92.92% production line coverage.

The running native Cockpit passed the passive controller catalogue and
delayed-preview checks. Its installed HidHide driver again reported a
65,654-byte application list and Cockpit reported `changed: false`.
The test used the dev executable from
[prepared output verification](2026-11-08-prepared-output-heartbeat.md).
Local probe and test results are under
`.research/reactive/hidhide-volume-watch/`.

### Mount Manager access on 9 November 2026

On Windows build `10.0.26200.0`, the unelevated Cockpit account opened
`\\.\MountPointManager` with zero requested access. Opening the same device
with either `GENERIC_READ` or `FILE_READ_DATA` returned Win32 error 5.
`IOCTL_MOUNTMGR_CHANGE_NOTIFY` also returned error 5 on the zero-access
handle. This probe only opened the device and requested a notification.
It changed no mount points or HidHide policy.

Mount Manager documents this notification for its persistent symbolic-link
database. The separate drive-alias probe above used a session-local DOS
mapping whose change broadcast was suppressed. Those two boundaries keep
the documented notification from qualifying as a replacement for the
active executable audit on the normal Cockpit account. The HidHide
acquisition decision remains open in #189. [Mount Manager change notification](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/mountmgr/ni-mountmgr-ioctl_mountmgr_change_notify) · [DOS device mappings](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-definedosdevicew)

Local probe `mount-manager-access.ps1` SHA-256 is
`ce78c2c532bb06f2ceca3976e3d1e7ae82d65cbedb4315d7b8c874bf138c64f0`.
Output `mount-manager-access.json` SHA-256 is
`f74341e54cef0475e124d7151d5bbc1c6d748337f3cc5c3ee02922ec03b52f5d`.
Both are under `.research/reactive/hidhide-volume-watch/`.

## Driver readback limit

HidHide 1.5.230 sizes its list response using the byte length of each
UTF-16 string as a character count. The earlier policy validation only
counted the bytes Cockpit would write. That could admit a write whose
subsequent driver readback exceeded Cockpit's limit. Validation now uses
the pinned driver's reported size before admitting a policy mutation.
[Pinned HidHide list sizing](https://github.com/nefarius/HidHide/blob/v1.5.230.0/HidHide/src/Config.c#L675-L700)

The regression starts with a 65,506-byte reported application list. Adding
the capture executable would make the policy unreadable by Cockpit.
Startup self-access reports the size error before it opens a recovery or
mutation path. The fixture records zero writes and the original policy.
An older journal persisted before its first write remains readable and
restorable. Applying that plan as a new session fails before mutation.

All 173 controller tests and 38 production coverage checks passed.
The rebuilt Cockpit passed the two passive controller checks and reported
the installed driver's oversized-list refusal with `changed: false`.
Executable SHA-256
`38cc6d5a33b5677317c26f5f1452dc4877ceed28e881df0cd5000cd035a667bc`.
