# Automatic controller lifecycle acceptance

On October 13, 2026, Cockpit passed the CDC v3 automatic controller lifecycle
checks against independently running production gateway and device simulations.
New devices enabled without Add, slots or layout Apply. Enabled offline devices
retained neutral collections; Disable, Enable and offline Remove followed the
persisted lifecycle. CDC v2 and Pico retain their older controls.

## Automated checks

| Check | Result |
| --- | --- |
| Frontend transport and lifecycle tests | 48 passed |
| Rust native adapter and manager tests | 66 passed |
| Dependency bootstrap tests | 5 passed |
| Production gateway and device acceptance scenarios | 2 passed |
| Actual C formatter snapshots within native tests | All 11 CDC v3 cases passed |
| Svelte check and autofixer | No errors, warnings or issues |
| Vite production build | Passed |

The first process acceptance scenario started Cockpit before its gateway and
three JSON devices. It verified automatic enrollment, full configuration and
saved calibration, explicit profile refresh, live input, neutral offline ghosts
and device restart. Disable survived repeated announcements and a gateway
restart while preserving online configuration. Enable appended after the
surviving controllers without changing their relative order or report IDs.
Online Remove failed; offline Remove forgot the identity, and a returning
forgotten device appended automatically with its own saved settings.

Disabling held buttons, axes and hats produced neutral inventory. Disabling
every device left no joystick reports or joystick report descriptor, while CDC
configuration remained available. Continuing announcements preserved this
empty layout, and explicit Enable placed the first controller at descriptor
index zero. Closing or crashing Cockpit preserved independently running
firmware processes and enforced configuration ownership on restart.

The second process scenario used two gateways reporting the same device
identity. Configuration, reports, writer ownership and recovery remained
independent. Gateway B stayed responsive and writable while A stalled,
disconnected and crashed. Independent B polling continued during A's UID
recovery deadline, matching the application's polling behavior.

Native regressions also covered changed CDC paths, lost lifecycle and
configuration acknowledgements, interrupted activation readback, stale epochs,
different gateway identities, module reboot, flash failure, pending layouts,
removal races, all declared capacity bounds and explicit report IDs independent
of descriptor positions. Reconciliation used snapshots and configuration reads;
ambiguous mutations were sent once. Renewed CAN routes were checked with bounded
reads before starting setup, and unavailable routes never replayed a write.

## Rendered interface

An isolated browser smoke check used the actual Svelte application with a
deterministic gateway response fixture. The gateway inventory displayed an
enabled online controller, a grey offline retained controller and a remembered
disabled online device in descriptor order. It showed Enable and Disable without
legacy Add or layout Apply controls.

The enabled device's runtime editor described explicit Update profile after
changed controls. The disabled online editor remained available and explained
that saved module controls are used on its next explicit Enable. Its setup
button remained available while its USB collection was absent.

Removal and Disable confirmations displayed the required text:

> This reconnects the gateway and may require rebinding remaining controllers in your games.

When the offline device returned during its open removal dialog, the UI explained
the change and disabled confirmation. No Forget request was sent. Explicit
Enable and confirmed Disable each sent one lifecycle command. Pending Disable
displayed that its previous USB collection remained published; the device page
reserved the absent-controller explanation for an actually unpublished device.
The browser reported no warnings or errors.

## Reproduction and scope

The local process run used sanitized native gateway and device builds, the
ordinary Rust gateway service and the repository's `tests/e2e/acceptance.mjs`.
Builds and evidence were retained under the workspace's
`.research/controller-lifecycle/` directory, including `acceptance-final.log`,
the per-scenario `result.json` records, native/frontend/bootstrap logs and UI
screenshots. The [E2E guide](../../tests/e2e/README.md) describes provisioning,
source overrides and a custom output directory for reproduction.

Local streams exercise production CAN handlers, settings, persistence,
controller layouts and reports, but do not create Windows USB/HID devices.
Physical descriptor changes, game binding retention and module movement between
gateways remain hardware qualification work. No release versions were changed.
