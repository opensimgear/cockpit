# Windows vJoy M3 qualification — 15 October 2026

Issue #25 remains open, **1/6 acceptance criteria verified**. Windows hardware
qualification resumed after verified reboot restoration. The earlier blocked
checkpoint below is retained as history. No production qualification gate was enabled.

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

## Post-reboot native provisioning and activation

The user rebooted Windows. The saved read-only restoration script passed: boot UTC
`2026-10-15T12:53:11.5000000Z`, original FREE eight-button/non-FFB slot 1, unchanged
OEM registration, valid driver signatures and exact original HidHide policy. The
M0 task was notified and hardware mutation ownership remains with this #25 task.

The corrected production provisioning dispatcher completed both explicit requests:

| Slot | Axes | Buttons | Continuous hats | Result |
| --- | --- | --- | --- | --- |
| 1 | 16 | 8 | 0 | Original implicit shape retained as a persistent descriptor |
| 2 | 2 (X/Y) | 8 | 1 | Native creation, protected committed journal and readback passed |
| 3 | 3 (X/Y/Z) | 16 | 2 | Native creation, protected committed journal and readback passed |

All slots are non-FFB. The operations completed in 1,273 ms and 1,380 ms respectively.
Independent inspection and Windows joystick API enumeration agree with the new
layouts. The slot-3 plan starts from the persisted slot-1/2 configuration, so it
also exercises retention of existing custom descriptors.

Commit `8e6ad4c799d0ae91537f29d39a8774a3edeb0129` passed all 11 CI jobs in run
[38732244382](https://github.com/opensimgear/cockpit/actions/runs/38732244382).
Downloaded Windows artifacts show native writer coverage 367/381 lines (96.33%),
descriptor coverage 59/59 (100%), and both new regression tests passed.

A separate scratch host calls the production `Service::with_output` with an
explicit qualification-only enablement, persisted physical source settings,
profile/assignments/target storage, mapping clock and normal feeder factory. Only
the private constructor's visibility was changed in the scratch copy. Ordinary
Cockpit continues to construct that service with `qualified = false`; provisioning,
script, Linux/macOS and visibility qualification constants remain false.

The isolated profile maps the right Gladiator's X/Y axes, button 1 and hat to both
custom targets. No hiding is requested. The original source remains independently
enumerated as VID 231D/PID 0200. The physical profile uses unipolar 0–32767 axes and
fallback 16384; the qualification fixture initially supplied an out-of-range 32768
fallback, which validation rejected before output and was corrected in the fixture.

### Real activation exceeded the report deadline

The production factory's 100 ms response deadline killed the first two-slot
activation before acknowledgement. Its durable phase remained `intent`; fresh
inspection found both slots FREE. An independent bounded feeder measurement then
recorded successful native Start in **187 ms**, Stop in less than 1 ms, and clean
process exit. That measurement sent only neutral fallback states, without movement.

The parent now gives Start/Recover the original bounded startup budget (three
seconds in the native factory), retaining the short report/renewal response budget.
Regression coverage exercises a slow successful Start and Recover, a delayed
renewal that still terminates promptly, and activation that exceeds its bounded
budget. All **81 Windows** and **51 Linux** virtual-controller tests passed locally.
The ordinary report/lease constants remain 100 ms response, 50 ms renewal and
250 ms feeder lease. This does not qualify maximum-target latency or lease timing.

After explicit acknowledgement of the inspected unowned intent, a fresh host
loaded the same source and both target assignments, started both slots successfully,
and reported healthy live output with no recovery fault. An independent read-only
Windows joystick observer was started before the manual movement cue. Physical
movement, game bindings and the rest of the composite acceptance checks remain
pending until their actual evidence is recorded.

Additional raw evidence SHA-256:

- `post-reboot-create2.jsonl`: `1e3cf1df1c21536738eee69cf5373675d96458522a2f906fa1a68d436fe2d67a`
- `post-reboot-create3.jsonl`: `0d4e6bca49f08c68c77ccaa3c30285c031e6de4c7e270a3aa48a694040d6baa4`
- M0 `windows-followup-after.json`: `1b60ccbc02f64ed18f0c92100a3bb502e0e78b495adfdf4862ffc341bd78c595`
- `vjoy-native-feeder-timing.json`: `f3bae43f628b36865da81e4e5c756b6e0886a2de478c37aeadb7736b2d7c5e04`
