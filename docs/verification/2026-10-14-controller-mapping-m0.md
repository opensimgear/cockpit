# Controller mapping M0 qualification, 14 October 2026

Status: **partial; #21 remains open**. No controller-mapping production feature is
enabled by this work. The Ready queue in OpenSimGear project 2 contains Cockpit
#21–#33. Their prerequisite order begins with #21; this record does not mark its
dependent milestones complete.

## Recorded machine and artifacts

Windows 11 Pro x64, build 10.0.26200. Linux tests ran in Ubuntu 24.04.4 LTS x86_64
on WSL2 kernel 6.18.33.2-microsoft-standard-WSL2. Rust 1.90.0; frontend validation
used the configured Linux Node 26.11.1. These observations are not a supported-OS
matrix or a latency benchmark.

The user requested an update from vJoy 2.1.9.1 to the plan's selected
[vJoy 2.2.2.0](https://github.com/BrunnerInnovation/vJoy/releases/tag/v2.2.2.0).
MOZA RacingLab initially owned slot 1. The user closed it and the slot was checked
FREE before installation. The installer signature validated as Brunner Elektronik
AG. Installation exited 0 and reported no reboot needed. The running driver's
signature validated as Microsoft Windows Hardware Compatibility Publisher.
No signing protections were disabled.

Driver/DLL APIs both report 0x222 and DriverMatch succeeds. A complete V3 report is
124 bytes with four-byte alignment; one byte of device ID is followed by padding
and thirty 32-bit words. The release source is
[bd268d1fa5934a900a79e76e519d4aa804e0ee3d](https://github.com/BrunnerInnovation/vJoy/tree/bd268d1fa5934a900a79e76e519d4aa804e0ee3d).
Do not reuse a V2-size report with this release.

The selected [HidHide 1.5.230 release](https://github.com/nefarius/HidHide/releases/tag/v1.5.230.0)
ships driver binary version 1.4.181.0. The installed driver's SHA-256 exactly
matches that release's Git LFS artifact, and its Microsoft signature validates.
The installed CLI reports 1.5.230.0; the differing component versions do **not**
require a driver update.

| Artifact | SHA-256 |
| --- | --- |
| vJoySetup_v2.2.2.0_Win10_Win11.exe | ef569a3105cd301b89580f18f60c66b339e95296acf2c0dfcaf4b4bbf8ab68fe |
| vJoy 2.2.2.0 SDK.zip | 0e796b185b66819d5fbeae645f3f038ecbfbbde837d3d3f06cba82ae1db07c67 |
| Installed vJoyInterface.dll x64 | 199316aaefac95118e869c07e7f332ce492dd9a8b6dac5a3ca20b1d5bb5058a1 |
| Installed vjoy.sys 2.2.2.0 | 540ac46bc529001798068bd402bb4c36891d4c0e63b9dac9761b14bd38667d7d |
| HidHide.sys 1.4.181.0, release/installed match | e3bb8b6a4e4ff90290b34e710edbef88cf71cec6ba0cadcb8e1685d5dd65ddb2 |
| Installed HidHideCLI.exe 1.5.230.0 | 9dd283fedfbd301e1a574a3d4b8663f6274ccb5c896f468ed55a6239f9ade270 |

The SDK/archive hashes record the downloaded official artifacts; unlike the
installer/driver signature checks, a locally calculated hash alone does not prove
publisher identity. Rhai 1.26.1 and all transitive dependencies are pinned by the
research crate's Cargo.lock.

## Executed checks

| Check | Result and limit |
| --- | --- |
| Protocol/readiness/visibility/evdev fixtures | 17 Python tests passed. Prototypes only; no production adapter. |
| Rhai bounds | Six Rust tests passed, including infinite loop termination, recursion limit, progress cancellation, syntax rejection, missing host APIs and repeatable simple evaluation. |
| Rhai operation probe | Infinite loop terminated at the configured operation budget; one observed debug run took 1,116 microseconds. Not a latency qualification. |
| Frontend check | Zero errors and zero warnings. |
| Transport regressions | 128 passed. |
| Native regressions | 238 library tests plus one service test passed; no failures. Existing non-Windows unused-code warnings remain. |
| UI build | Passed. |
| Native startup without Windows dependencies | Normal Linux service answered a correlated status request and shut down successfully. This is service startup, not a full desktop UI or newly integrated mapping feature check. |
| Windows controller capture | Message-only Raw Input window registered joystick/gamepad/multi-axis input sinks. Eight collections enumerated, including vJoy. Two VID 231D collections each delivered 62 reports in a five-second sample; other collections included software OpenSimGear inputs. |
| Foreground-game capture | Passed basic background read: after the user foregrounded Assetto Corsa Rally, the probe recorded foreground process acr and received 62 reports from each VID 231D collection in 5,006 ms. No user-control transition, mapping, latency or game-binding claim. |
| vJoy report ABI/readback | FREE slot 1 acquired; existing complete report resubmitted unchanged through UpdateVJD; every declared field matched GetPosition readback; slot relinquished in finally. No generated button pulses/game-binding test. |
| Update configuration backup | Service parameters and OEM force-feedback registration exported locally before installation. No explicit custom slot descriptors were present. OEM force-feedback registration is byte-for-byte unchanged afterward. |
| Live slot provisioning | Pending. The installer update and a report roundtrip do not establish safe create/edit/delete or implicit-default preservation by vJoyConfig. |
| HidHide visibility/session lifecycle | Pending. Current policy was read only. No physical source was hidden or global settings changed by these probes. |
| Linux readiness | No /dev/input/event nodes; ordinary-user /dev/uinput open denied with EACCES. A disposable user/mount namespace could be entered; this is not controlled game-launch isolation. |
| macOS | Unavailable: no native Mac, signing authorization or helper deployment was tested. |

Native inputs on this machine include two devices with VID 231D and PIDs 0200/011F.
No vendor model/firmware compatibility claim follows from those IDs. Windows
Raw Input device names are hashed in local experiment output; personal device paths,
registry exports and full machine logs are not checked in.

## API findings that constrain implementation

- vJoy's GetVJDAxisExist is declared BOOL but its release source can return negative
  error values, including an unavailable axis. Treat only 1 as success and report
  negative results separately; ordinary nonzero-to-bool conversion falsely advertises
  controls. The read-only probe was corrected before recording final capabilities.
- The observed post-update slot has eight buttons, no POVs and usages 0x30–0x38.
  The pre-update library did not provide a trustworthy capability snapshot while the
  slot was owned; do not claim that all default layouts were proven unchanged.
- HidHide's released Logic.c handles persistent GET/SET whitelist, blacklist,
  active and inverse operations. Logon-session IDs in this source are not
  process-owned cleanup. Newer master-branch session APIs remain unqualified for
  the selected signed binary.
- The existing HidHide policy has externally managed devices and application
  exceptions. The prototype rejects an incompatible global policy instead of
  rewriting those settings. A live test needs a reviewed capture exception and
  selected physical test instance, with journaled restoration.
- Apple's [HIDVirtualDevice](https://developer.apple.com/documentation/corehid/hidvirtualdevice)
  is available from macOS 15. The
  [virtual-device entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.hid.virtual.device)
  exists from macOS 10.15, which does not imply CoreHID availability or an authorized
  deployable helper on that older OS. Keep virtual creation unavailable until native
  signing/permissions/joystick recognition are qualified.
- Linux evdev grabs and macOS device seizure must be labelled input suppression,
  not enumeration hiding. WSL namespace creation alone does not qualify Steam,
  Proton, brokers, inherited descriptors, raw USB or hotplug isolation.

## Remaining M0 gates

1. Extend the passed foreground-game background-read check with controlled physical transitions and exact model/firmware records before broader capture qualification.
2. Prove provisioning preserves explicit and implicit layouts plus FFB registration;
   exercise cancellation/failure restoration without taking over a BUSY slot.
3. Qualify normal/inverse HidHide visibility with capture and non-allowlisted
   processes, executable-path behavior, external-change conflicts and actual cleanup.
4. Complete Windows setup/restart detection and physical Linux/macOS qualification
   on available native systems. Record unsupported modes as unavailable.
5. Promote the contract candidate only after these findings have been resolved
   or explicitly scoped by the project; do not close #21 based on mocks alone.

The research sources are deliberately separated from application code. No production
module changed, so this slice does not claim a new-production coverage percentage.
Later implementation milestones must enforce the plan's 71% measurable coverage gates.
