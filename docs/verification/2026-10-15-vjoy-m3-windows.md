# Windows vJoy M3 qualification — 15 October 2026

Issue #25 remains open, **1/6 acceptance criteria verified**. Windows hardware
qualification is waiting for restoration after the concurrent M0 test's required
reboot. No production qualification gate was enabled.

## Native custom-layout defect and correction

A separate scratch executable called the production native provisioning dispatcher,
launcher ticket, cancellation monitor, exclusive mutex, protected journal, registry
transaction and driver refresh code. The scratch copy changed only visibility of
`windows_provision_entry::native` and `Ticket::start` so the harness could call them;
it did not change either gate or the provisioning algorithm. It is not the Cockpit
UI and does not qualify the complete UI/game acceptance criterion.

Initial read-only inspection found signed, pinned vJoy 2.2.2.0, one FREE implicit
slot with 16 axes, eight buttons and no hats/FFB, no pending journal, and only Wdf
under Parameters. A registry backup was saved. Games/feeders were closed by the
user, who also confirmed Joystick Gremlin is installed.

The explicit request for slot 2 (two axes, eight buttons, one continuous hat)
failed: the planner accepted the layout, but `NativeBackend::mutate_create` still
required the legacy descriptor and hard-coded its length as 123 bytes. The helper
reported failure and restoration; its immediate readback matched the initial
layout and OEM fingerprint. This is failure-path evidence, not successful custom
provisioning or isolation from concurrent tests.

The writer now validates the exact canonical bounded descriptor grammar, checks
the supplied size, and writes the actual byte length. Existing journal intent,
non-overwrite checks and atomic ownership remain in place. New tests cover valid
custom layouts, wrong slot/size, missing or malformed descriptors, unknown fields,
and transactional custom layout creation with retained existing slots.

Validation: 80 Windows virtual-controller tests and 50 Linux portable tests passed.
This PC reports Windows error 6801 for isolated HKCU transactions; the transaction
test follows the existing explicit diagnostic/skip convention. Its pure descriptor
validation assertions ran. Native SYSTEM-hive success and CI coverage must be
verified separately; a passing local suite is not a hardware qualification claim.

## Concurrent hardware test and reboot checkpoint

The corrected attempt stopped before elevation/mutation because fresh inventory
reported slot 1 with 12 buttons and FFB. The independent SDK probe agreed. The
user authorized coordination with other Cockpit tasks. “Review issue 21” confirmed
its custom/FFB test caused this change and encountered a Windows reboot-required
condition. That task removed its test registry keys and verified unchanged OEM
registration and restored HidHide policy, but the temporary runtime layout remains
loaded until reboot. Its probes have exited and further hardware mutations are
paused. The installer task reported software-only work, with no installer run.

See the separate M0 Windows qualification record and its saved recovery journal.
After the user reboots, run the read-only
`.research/controller-mapping-m0/check-windows-followup.ps1 -Phase After` in
PowerShell 7. Require the original FREE eight-button/non-FFB layout, a changed boot
time, unchanged OEM registration, and exact original HidHide policy before resuming.
This task owns subsequent #25 vJoy hardware work; other tasks must avoid concurrent
driver, feeder, refresh or hiding tests.

## Remaining acceptance

Only the previously verified missing/disabled-dependency criterion is checked.
The corrected production provisioning path still needs two persistent custom
slots, physical button/axis/hat routing without HidHide, a real game binding check,
and Cockpit restart/layout/assignment/binding retention. Real Gremlin contention,
cancellation/failure/refresh/partial acquisition, source loss/stop/stale lease/feeder
death, and separate whole-process termination results remain open. Prompt the user
before manual movement, game, contention and reboot steps. Do not silently replace
or disable the temporary FFB layout to bypass preflight.

Raw evidence remains private under `.research/review-audit/`. SHA-256:

- `create2.jsonl`: `dab9624a6b4a37d7fd4973823b72e7feff811611abe60a1c66fae30417b3c147`
- `create2-fixed.jsonl`: `8f951d49cf6dd62d018afcdf8b933bf0d5a2bb9dc646e23bad629e33ec6c7670`
- `vjoy-before-qualification.reg`: `56cebbbff9ab65b5660a3c3e76a6207bc0652b36ebf90a2fe706ed3f3704594e`
