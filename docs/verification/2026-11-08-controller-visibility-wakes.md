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
