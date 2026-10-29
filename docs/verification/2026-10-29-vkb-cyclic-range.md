# VKB cyclic range and device identity

Checked the running native Cockpit with DCS Feature Lab revision 9 on
2026-10-29. The user swept the right VKB cyclic fully left and right,
holding its endpoints. The springless stick's resting position was not used
as a calibration center.

## Observed range

The assigned source is `VKBsim Gladiator EVO R`, VID `231d`, PID `0200`.
Its HID X field is usage `0x30`, report 1, data index 0, with a declared
native range of 0–4095 and a 16-bit field.

During the full-travel capture:

- Native X reported 0–2048.
- The physical controller display reported 0–16387 on its normalized
  0–32767 scale.
- The profile's virtual X input reported −32768 to −1 after its configured
  0–4095 range map and 2% deadzone.
- The same profile owner remained Running without degradation or fault.

Windows raw HID data is already limited before profile processing. The
profile consumes native values, not the physical display's auxiliary
normalized value, so there is no second normalization in this path.
Calibration or a programmed device axis limit requires checking in the
VKB configuration utility before identifying the device-side cause.
[VKB's technical support guidance](https://vkb-sim.com.au/pages/technical-support)
directs calibration through its own utility.

The local capture is retained in `.research/cyclic-axis/live-capture.json`.

## Physical block identity

Physical controller blocks now display the assigned hardware name below
their profile label. Offline observations keep their known device names;
unassigned and pending assignments are labeled separately. The existing
block dimensions, port anchors, and accessible profile labels remain in use.

The native UI check passed against the actual right VKB and active profile,
including a check that the expanded heading stays above the port anchors.
Its screenshot is saved locally as
`.research/cyclic-axis/physical-device-label.png`.

The identity helper's six tests cover exact device matching, whitespace,
offline identity, pending inventory, unassigned blocks, and other block
types. Line, branch, and function coverage each reached 100%.
The isolated patch snapshot passed all 867 profile UI tests and Svelte
checks with zero errors or warnings.
