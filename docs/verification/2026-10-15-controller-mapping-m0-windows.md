# Windows M0 qualification follow-up — 15 October 2026

Status: **in progress; Windows reboot required**. This record extends the original
[M0 evidence](2026-10-14-controller-mapping-m0.md); it does not close #21 or approve
the release contract. No production capability was enabled.

## Verified on the installed Windows drivers

- vJoy driver/DLL 0x222 match; initial slot 1 FREE, 16 axes, eight buttons, no POVs
  or FFB. The original registry contained only the Wdf parameter subkey.
- Both installed vJoy and HidHide driver signatures validated. Initial HidHide
  state: active normal mode, four application entries, three hidden devices.
- The existing implicit-default provisioning helper ran in Fail mode under Windows
  administrator authorization. Its injected error occurred after provisioning;
  restoration returned the implicit layout and preserved the exact OEM fingerprint.
  The independent guardian confirmed restoration.
- A new bounded inverse-mode helper tested HID VID 231D/PID 0200, the selected VKB
  Gladiator EVO R. The user reports firmware v2.20c and direct USB connection.
  The helper temporarily replaced the global policy with the user's authorization.
  The capture executable retained direct HID access, DirectInput enumeration and
  Raw Input reports; the explicitly blocked observer executable lost direct HID
  access (Win32 error 5) and DirectInput enumeration. Raw Input still delivered
  observer reports; this does not qualify universal input isolation.
- Inverse normal completion, injected exception (exit 1), and forced process exit
  (exit 86) each restored the exact original lists and active/inverse flags. The
  guardian recorded recovery for each run. A separate fresh verification compared
  the live driver with all three journals' original policies.
- The inverse helper passed the existing eight journal assertions plus checks for
  all six inverse stages, unique recovery-stage identification, one-setting
  transitions, and rejection of an unrecognized state. Live concurrent external
  changes, additional game APIs and executable aliases remain unqualified.

## Custom/FFB qualification encountered a real restart gate

The selected release's descriptor header and SDK constants generated a representative
custom descriptor: 12 buttons, 16 axes, no POVs, FFB enabled. The button-field width
remains 128 bits. Descriptor length: 1,330 bytes. SHA-256:
`cfa5be58ff0678d981d713ec6b5a1a2b61894dfb35a38aa488d78ab75b3ae39c`.

The driver successfully restarted into this explicit layout; native capabilities
confirmed 12 buttons and FFB support. The subsequent device restart, after staging
a temporary second slot, was refused by Windows:

> Device is pending system reboot to complete a previous operation.

The helper and its guardian could not finish live restoration because PnP refused
further restarts. Both test descriptor keys were removed and the original OEM
fingerprint was verified unchanged. **The currently running driver still exposes
the temporary 12-button FFB layout until Windows reboots.** Registry cleanup alone
is not successful runtime restoration. No process, driver service, or reboot was
forced to bypass this gate. The journals and failed-run logs are retained.

The custom-baseline normal/failure/cancellation preservation suite therefore
**has not passed**. The successful custom-layout load alone does not satisfy
criterion 5. Real FFB effect output was not exercised.

## Reboot checkpoint and continuation

The pre-reboot snapshot is saved in
`.research/controller-mapping-m0/windows-followup-before.json`. It records boot UTC
`2026-10-15T06:05:20.5000000Z`, the current runtime layout, driver hashes/signatures,
clean descriptor-key state, unchanged OEM fingerprint and exact HidHide restoration.
Raw machine evidence and journals are retained under
`.research/controller-mapping-m0/windows-followup-evidence/`; these stay ignored.
The original local Windows evidence directory is saved in
`.research/controller-mapping-m0/windows-followup-directory.txt`.

After the user saves work, reboots Windows and returns to this chat, run the
read-only `check-windows-followup.ps1 -Phase After` in PowerShell 7 outside the
sandbox, with process-only execution policy if needed. It requires a changed boot
time, matching driver/DLL versions, the original FREE eight-button non-FFB slot,
no test descriptors, unchanged OEM fingerprint, valid signatures and exact
HidHide baseline. Preserve its result before continuing tests.

Then finish custom/FFB preservation using a reboot-aware protocol: treat restart
required as an incomplete transaction and retain recovery evidence across reboot.
Do not rerun the current continuous-restart suite blindly. Complete real external
conflict/path/API qualification where supported; perform controlled physical
transitions with a game foreground when the user is ready. Keep Linux/macOS
qualification and final contract approval separate and open.

## Coordination checkpoint

This chat has paused all vJoy/HidHide mutations while the #25 Windows qualification
chat coordinates shared hardware access. The #21 probes caused the observed
16-axis/eight-button/non-FFB -> 16-axis/12-button/FFB runtime change. The successful
fixture restart was followed by PnP refusing subsequent restarts as reboot-required.
The suite and guardian have exited. Last saved state at 12:02:20 UTC: both test
descriptor keys removed (Parameters contains Wdf only), original OEM fingerprint
unchanged, exact original HidHide policy restored, runtime vJoy slot 1 still FREE
with 12 buttons and FFB until reboot. No more provisioning/refresh/feeder/hiding
operations will be started here before coordination and reboot verification.
