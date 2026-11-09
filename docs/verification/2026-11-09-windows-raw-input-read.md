# Windows controller Raw Input reads

The Windows capture owner now supplies a reusable, aligned 256 KiB buffer to
`GetRawInputData` and parses the returned byte count. This removes the separate
size query and allocation per Windows message. The HID decoder reads each
validated report in place and reuses its result array. It clears that array
before decode because a button can write only the one-byte `On` member. The
decoded map is prepared before taking the registry lock. Ordered batches and
the existing packet and report limits still govern admission.
[GetRawInputData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputdata)
· [HidP_GetData](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidpi/nf-hidpi-hidp_getdata)
· [HIDP_DATA](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidpi/ns-hidpi-_hidp_data)

The Windows fixtures prove one API call per message into the same buffer,
ordered reports across distinct report IDs, a clean button value after a
wide axis, and rejection of a truncated packet after a successful read.
All 176 controller tests and 183 mapping-host tests passed. The changed
production module reached 98.25% line and 86.21% function coverage.

## Running Windows Cockpit

The isolated dev app passed the Devices preview hide and reveal check,
visibility reader retirement, and shared capture with an applied output-free
profile. The profile continued to receive the known value with its preview
closed. Reopening took a fresh baseline. Profile replacement and Stop cleared
the retired owner. The profile fixture used revision 1 with semantic hash
`sha256:8c220d7f7c771227a8980cc2bca60e8af986336de02aea1cca56fb100b9f2879`.

The stationary VKBsim Gladiator EVO R exposed 137 controls. Over ten seconds
the Devices preview matched 125 consecutive reports, with 80.004 ms median
between journal commits. Journal-to-DOM p50/p95/p99 was 7.04/8.18/8.75 ms with
a ±2.15 ms clock bracket. This check starts after native decode and reports
DOM updates. A deliberate physical sweep, screen presentation and
raw-input-to-virtual receipt remain separate in #199.

The dev app SHA-256 is
`ff97e6af856398060cf39c0e0e83ba41baec761423a22587abce5b20f7517740`.
The local timing file is
`.research/c193-reconnect/raw-input-read/controller-preview-latency.json`,
SHA-256 `4af3757cedd6d0e2531f485c6395c56bac0d3ce0d108caec2816eed962b6e926`.
The profile evidence is in
`.research/reactive/profile-source-wakes-native/1794199561897/evidence.json`.
