# Attached game-controller input corpus (#178)

These are actual passive captures from two attached physical game controllers on Windows x64 on 2026-11-28. They are **idle input evidence**, not transition, full-travel or hardware-output qualification. No controls were actuated by this task.

| Fixture | VID:PID | Raw input reports | Cockpit snapshots | OS-parsed controls |
| --- | --- | ---: | ---: | ---: |
| `231d-011f-idle.json` — VKBSim T-Rudder | 231d:011f | 63 | 76 | 10 |
| `231d-0200-idle.json` — VKBsim Gladiator EVO R | 231d:0200 | 62 | 76 | 137 |

Each device has one unique 64-byte input report and one unique parsed value state in its respective window. Sequence numbers advanced while values remained unchanged. Zero observed changes means these fixtures exercise idle reports only; it does not establish neutral calibration, button edges, hats, sign changes or axis endpoints. Nonzero idle axis values are preserved exactly.

The raw payloads are `RAWHID.bRawData` delivered by Windows `WM_INPUT`, copied using `GetRawInputData`. They are real input-report bytes, not reconstructed from normalized controls. They are not a USB bus trace. Raw HID **report descriptor bytes were not available/collected**. `descriptor.osParsedCapabilities` is the running Cockpit's Windows HidP capability projection, including its diagnostic entries; it must not be used as proof that the raw descriptor parser accepts these devices. The raw and parsed windows are separate and not packet-synchronized; no raw-to-parsed decoding equivalence is asserted.

Machine device paths, serial/fingerprint, stable controller IDs, DirectInput instance IDs and foreground process are absent from fixtures. Retained product names and VID/PID identify the product families, not the user's machine. The collector registers only Generic Desktop joystick/gamepad/multi-axis TLC usages 4/5/8, excludes known vJoy 1234:bead, and copies reports only for the enumerated non-vJoy controller handles. The two retained products were independently confirmed by the actual app inventory. Keyboard/mouse usages are never registered or retained. Final corpus mode opens no device files, sends no HID feature/output request, injects no input and performs no physical actuation.

## Verify the archived evidence

From the Cockpit root:

```powershell
node tests/hid-device-captures/verify.mjs
```

This offline check validates archived report counts, known report-byte SHA-256 values, parsed-state hashes, control/sample bounds, state references, monotonic timestamps, privacy redactions, idle classification and the project file-line limit. It contacts no hardware. SHA-256 values apply to decoded input-report bytes, not their hexadecimal text:

- T-Rudder: `ca37f5c15ecf546bbd41c111effe599fbca9a1db13443167e64df52d392d5268`
- Gladiator EVO R: `68c7c8f9d5ac5d7c60856114402a8180767b2c6dd13ef440369f6acf3c8220bb`

## Reproduce a candidate capture

Use a Windows interactive **host desktop session**, not a sandbox token that denies controller handles or hides desktop processes. Leave physical inputs untouched. Use the running native development Cockpit with Vite at 1420 and existing CDP at 9222. `capture-live.mjs` refuses to continue if any profile owns outputs; it never stops a profile automatically. No configuration changes or simulated input are needed.

```powershell
# Compiles the maintained repository probe, not a generated C# variant.
& ./tests/hid-device-captures/capture-raw.ps1
node tests/hid-device-captures/capture-live.mjs
node tests/hid-device-captures/build-corpus.mjs
node tests/hid-device-captures/verify.mjs --candidate
node tests/hid-device-captures/verify-live.mjs
```

`RawInputProbe.cs --physical-corpus` is opt-in; its earlier modes keep their previous behavior. This mode has a fixed five-second window, maximum 512 retained reports and 4096 bytes/report, a 256-device enumeration bound and explicit overflow rejection. Each RAWHID batch retains the single timestamp taken at entry to its `WM_INPUT` handler. Serializer capacity covers the report cap. Raw input and source matching intermediates stay under ignored `.generated/`; candidate fixture files are written to `.generated/candidates/`. The parsed observer takes six seconds with at most eight sources and 128 snapshots/source, then closes every reader.

Recapture counts and values naturally vary. Candidate validation checks structure, hashes and bounds without demanding archived counts/values. Review candidates and their idle/transition classification explicitly before replacing an archived fixture; the scripts never overwrite archived fixture JSON. These files cover the two specified products; a changed VID/PID/interface set requires a reviewed mapping rather than implicit identity guessing. `verify-live.mjs` checks both product labels in the actual Devices view and writes `.generated/live-ui.json`, preserving the archived `live-ui.json` receipt.

## Verification record and limits

The actual native app showed both products in Devices; its native controller observers reached `ready` for both. Capture checked `profile_status` before and after: both reads returned a null owner. Live UI evidence is recorded in `live-ui.json`. Source state at investigation start was committed HEAD `30af99482fd213d86c044413a22ccaeb411f7b24` plus a substantial pre-existing dirty checkout, which was preserved. An existing development binary was reused, not rebuilt from the dirty source; its SHA-256 was `4375933ea3570d05a16d6fc50644b40510b1da18e365d81de9e3262e5bf4ea95`. Accordingly, parsed samples document that running binary's behavior, not acceptance of subsequently changed decoder source.

Final maintained collector was compiled and run on the host, then both candidate and archived verification passed. Physical button/hat/axis transitions, reconnect/reset, additional devices/report IDs, Linux/macOS, raw descriptor decoding, feature reports and output behavior remain unqualified. No synthetic transitions have been presented as real-device evidence.
