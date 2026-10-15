# Windows vJoy M3 qualification — 15 October 2026

Issue #25 is accepted for closure at the user's request, with **6/6 listed
acceptance criteria verified**. Remaining reliability and production enablement
qualification is carried into #33; no production qualification gate was enabled. Windows hardware
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

At the initial checkpoint only the missing/disabled-dependency criterion was checked.
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

## Physical X/Y, trigger and hat routing passed

The user completed the cued right-Gladiator movements with games closed. The
independent read-only Windows joystick observer recorded the original physical
VID 231D/PID 0200 device and both virtual VID 1234/PID BEAD devices simultaneously.
The two-axis/eight-button device identifies slot 2; the three-axis/sixteen-button
device identifies slot 3. The untouched six-axis API view of slot 1 was also present.

| Observed device | Changed samples | X range | Y range | Mapped button | POV |
| --- | --- | --- | --- | --- | --- |
| Physical right Gladiator | 667 | 0–65535 | 0–65535 | Press/release observed | All eight directions and neutral |
| vJoy slot 2 | 662 | 0–65535 | 0–65535 | Button 1 press/release | All eight directions and neutral |
| vJoy slot 3 | 661 | 0–65535 | 0–65535 | Button 1 press/release | All eight directions and neutral |

These are the joystick API's scaled axis values, not Cockpit's 0–32767 normalized
range. Every recorded API read succeeded. Slot 1 produced only its initial state;
no subsequent observed state changed. The production host remained running and
ready with no fault or recovery requirement after the manual phase. It had recorded
6,288 ordered mapping reports by the status check. No device hiding was requested;
the original HidHide policy was preserved. This observer result establishes routing,
not real-game binding or universal API visibility.

The normal Windows Tauri application's own read-only status was separately checked:
virtual devices `enabled=false`, `qualified=false`; provisioning and scripts
unqualified; device hiding disabled; no live mapping or output recovery active.
The isolated qualification host's explicit live enablement is confined to its
separate executable and local test storage.

Commit `ee1811e80153f844728e30b8eb1f2eb930ea72a4` passed native virtual-controller
tests/coverage in CI run 38741797150, but that run failed overall: the desktop
native-service integration test expected two catalogue entries while the concurrently
merged X-Plane integration now provides four. The owning task was notified with the
exact Windows/macOS failure. Do not report this checkpoint's complete CI as passing.

The user has been asked to bind a virtual axis, button and hat in a real game,
explicitly checking that the binding source is vJoy and the original remains listed.
That manual result is pending, so acceptance criterion 1 remains unchecked.

Finalized raw evidence SHA-256:

- `vjoy-winmm-routing.jsonl`: `400e7074083b0bab806060ba1230e6e4208b0c47e7a14c913e7cb77c2f9fa431`
- `vjoy-routing-summary.json`: `18b89141d8551daf5c01dcc5ebb2230139e0642ef994b80f25ba83fcd44d2051`

## Real ACR bindings and application retention verified

ACR binds on movement without identifying the controller in its capture UI. To
avoid attributing simultaneous physical/virtual movement to the wrong source, an
explicit synthetic-source qualification profile drove **only slot 2**, with physical
routing stopped. With ACR foreground and its binding capture armed by the user,
three separately cued gestures were generated: X full travel and centre, button 1
press/release, and hat-up/neutral. Each gesture returned to neutral. The user confirmed
all three were captured and saved. The independent observer recorded those changes
on slot 2 while the original Gladiator stayed still during the gestures. One button
attempt refused to send because ACR was not foreground; the subsequent cued retry
succeeded. No hiding was used to make the game select a virtual input.

The isolated mapping host then stopped cleanly and restarted with the same saved
source/slot assignments. Saved profile and setting file hashes were unchanged.
The first physical retest failed because the feeder exited immediately after
activation and the supervisor recovered neutral output. This was investigated and
corrected before repeating the game test; the failed attempt is not counted as a pass.

The normal Windows Tauri application was also tested separately. A clearly named
qualification profile was imported through its production API with the real right
Gladiator source ID and slots 2/3 assigned. After a clean normal application restart,
the production API returned exactly the same source and target assignments. Both
before/after evidence documents have the same SHA-256:
`192090a71151f70bb525f256f65924bbbdbf0be1e0274979ff5c444559a25468`.
The normal app remained stopped with virtual devices disabled/unqualified,
provisioning/scripts unqualified and hiding disabled. Live qualification continues
only in the separate test host; this does not claim ordinary UI live output was enabled.

After the lease correction described below, the user retried the **same saved ACR
bindings without rebinding** using physical right-Gladiator input and confirmed:
“Steering and both other bindings work.” X was bound to steering; the other action
names were not supplied. The independent observer again recorded mapped axis,
button and hat changes on both virtual devices, and the host remained healthy,
ready and fault-free (2,768 recorded mapping reports at the check). The saved HidHide
baseline was decoded as UTF-16 MULTI_SZ and independently verified against the
live driver: neither VID 231D/PID 0200 nor either vJoy product identity is hidden.
The unrelated original hiding policy remains intact.

This completes acceptance criteria **1 and 3**, in addition to previously completed
criterion 2. Criteria 4–6 remain open. The user saved the bindings and closed ACR
before further ownership/failure testing.

Binding evidence SHA-256:

- `vjoy-winmm-game.jsonl`: `13cafb4380ab7382ee8c1a602ec31bd368b721819602914df1ec0ebe60b8bfeb`
- `vjoy-binding-gestures.jsonl`: `8b2c0322ae1eee265158a30b23b9d997f7e40e716fcbef76a54864d2ed471cec`

## Initial lease starts after native acquisition

Giving Start a separate IPC budget exposed another timing defect: the worker used
the pre-acquisition timestamp for the first 250 ms lease. Native acquisition could
consume that window, so a successfully acknowledged Start could expire immediately.
The failing restart attempt's observer timeline showed both slots return to neutral
just after activation. The surviving host persisted a clean recovered record and
stayed stopped; subsequent physical movement was not forwarded into the game.

A regression driver deliberately takes 300 ms to acquire its target. Before the
fix, the production worker rejected its already-queued first renewal with “Mapping
heartbeat expired; fallback submitted and ownership released.” The worker now arms
the first lease using a fresh timestamp after successful acquisition and before
acknowledging Start. The running lease remains 250 ms, renewal 50 ms and ordinary
IPC response budget 100 ms. No Report can reset the lease; existing stalled-pipe
expiry tests still pass. All **82 Windows** and **52 Linux** virtual-controller tests
passed. The corrected native host remained healthy beyond a minute of idle time
and through the user's successful ACR physical retest. Full CI for this correction
is pending until the checkpoint is pushed and its run completes.

The separately owned X-Plane CI test corrections are tracked independently:
`b5c85b9` fixes the stale two-game catalogue assertion; `3bdca2c` synchronizes UDP
fixture arrival; `7f26f3f` uses a bounded full-datagram peek buffer for Windows
WSAEMSGSIZE behavior. The owning task validated the last fix with nine provider
tests and 100 lifecycle repetitions on Windows and Linux. These fixture failures
must not be represented as passing overall checkpoint CI.

## Source loss, Stop and stale lease

The game remained closed for these checks. Independent WinMM observation recorded
the right Gladiator and both virtual slots while the isolated production mapping
host ran. Unplugging the held-trigger source produced source-lost recording entries
3899/3900 and fallback on both virtual slots at observer times 62,504/62,505 ms:
axes centred, buttons released and hats neutral. Reconnection at 82,302 ms did not
replay the held initial state; forwarding resumed from the fresh released baseline.
The mapping supervisor survived and reported no fault after reconnect.

For normal Stop, the user held trigger and hat-up. Both virtual slots first showed
button 1 pressed and POV up. The actual `mapping_stop` request then returned with
running false, no fault and no pending recovery. Both slots reached fallback at
545,541 ms while the physical controls were still held. A separate SDK inspection
confirmed slots 1, 2 and 3 free. The user was then cued to release the controls.

A separate bounded native Session probe deliberately withheld renewals after
submitting nonneutral reports. WinMM recorded both slots nonneutral at
843,122/843,123 ms and fallback at 843,371 ms. The feeder explicitly reported
heartbeat expiry; the surviving supervisor recovered successfully, left a clean
durable journal and verified both slots free. A requested seven-button layout
against the actual eight-button slot 2 was separately rejected with identical
before/after layouts and ownership. These probes used the unchanged production
250 ms lease; diagnostic logging did not change timing policy or production gates.

One earlier Stop attempt was deliberately not counted: before a Stop request was
sent, another spontaneous feeder exit had already returned output to fallback.
The host survived, but its cause remains under investigation. Scratch-only stderr
and host-loop-gap diagnostics were added before the successful Stop retry. This
unexplained exit and the outstanding deliberate feeder-death/whole-process and
ownership/provisioning checks keep the issue open and production output disabled.

Checkpoint `40a488591d34d211b1659332d9a4420b77c8df94` subsequently passed all
11 jobs in [CI run 38747838000](https://github.com/opensimgear/cockpit/actions/runs/38747838000),
including native controller jobs on Windows, Linux and macOS. The native hardware
results above remain separate from automated CI; criteria 4–6 remain unchecked.

## Deliberate feeder death and total-process termination

After the bounded previous host expired cleanly, a fresh diagnostic host (PID
37828) and read-only observer were started. With the user holding the right trigger
and hat-up, all three observed devices showed button 1 pressed and POV up. The
exact feeder child (PID 27016, verified parent, executable, command line and creation
time) was forcibly terminated. Its supervisor stayed alive. Both virtual devices
returned to centred axes, released buttons and neutral hats (observer times
88,922/89,047 ms) while the physical controls remained held. The host reported
“Output helper exited. Fallback recovery completed; mapping remains stopped.”
No recovery remained required; the journal was clean and an independent SDK query
confirmed all three slots free. The user was then cued to release the controls.
This completes the specified fallback cases in criterion 5. The separately noted
unexplained earlier exit remains an open reliability qualification gap.

Total-process termination was tested separately using a bounded native Session
probe. With independent observation confirming both test slots at maximum axes,
button 1 pressed and hat-up, the exact probe parent (PID 30036) and its feeder
(PID 6616) were forcibly terminated, leaving no supervisor to clean up. **vJoy
retained the nonneutral report after both processes exited.** The durable journal
remained `confirmed`, rather than falsely claiming cleanup. Explicit production
`Session::recover_stored` then restored centred axes, released buttons and neutral
hats (observer times 145,707/145,802 ms), released both slots and marked the journal
clean. No physical mapping or prior report was replayed. This records criterion 6;
it does not claim immediate neutralisation after total-process termination.

The probe avoids synchronous inventory inspection while maintaining a live lease:
inspection can take longer than 250 ms, so independent WinMM capture confirms the
live state instead. Recovery inspection runs only after output has stopped.
Evidence is retained in `.research/review-audit/vjoy-winmm-feeder-death.jsonl`,
`vjoy-terminated-feeder.json`, `vjoy-after-feeder-death-status.json`,
`vjoy-after-feeder-death-sdk.json`, `vjoy-whole-termination.json`,
`vjoy-whole-after-termination.json` and `vjoy-whole-after-recovery.json`, plus the
separate native probe journals. Production capability gates remain false.

Acceptance progress is now **5/6 criteria verified**: criterion 4's Gremlin
contention, partial acquisition and provisioning cases are still incomplete, and
the unexplained spontaneous feeder exit still needs investigation before closure.
The previous documentation checkpoint `ad184ee` passed its complete CI run
[38750392520](https://github.com/opensimgear/cockpit/actions/runs/38750392520).

## Gremlin contention, partial acquisition and cancellation

The installed Joystick Gremlin reports R13.3 at startup (Windows uninstall metadata
says 13.1.0). A separate temporary profile copies the existing profile's remap
grammar and directs its three right-Gladiator axes only to vJoy slot 3. The original
saved profile remained unchanged (SHA-256
`2ac9b5c30f7c422be37004d1401919d025276d3b77f04bb3235b76d5a8af3aac`).
The user loaded and activated the temporary profile. Gremlin acquires this output
lazily: the first physical X-axis movement made slot 3 BUSY, owned by Gremlin PID
31956, with slots 1/2 free. Production Session activation then rejected the two-slot
mapping with `Slot 3 is Busy; never take over another feeder`. Before/after layouts
and ownership were identical; Gremlin retained slot 3 and the probe's intent-only
journal was explicitly acknowledged without output recovery.

For partial acquisition, Gremlin was deactivated and an independent SDK query
confirmed all slots free. A scratch Driver wrapper around the actual native SDK
paused production `Feeder::receive(Start)` immediately before slot 3's second
inspection. The actual production preflight had checked both slots free; slot 2
was then acquired by the probe (PID 32928) and given its complete neutral report.
Gremlin was reactivated and the user moved X again. Independent inspection confirmed
slot 2 owned by the probe and slot 3 owned by Gremlin before the barrier resumed.
Production Feeder returned `A target became owned during acquisition`, submitted
fallback and released **only slot 2**. Its event log contains no acquisition,
submission or release of slot 3. A fresh SDK inspection confirmed slots 1/2 free
and slot 3 still owned by Gremlin. The temporary profile was then deactivated and
all slots were verified free. The barrier is bounded to 180 seconds and changes
only qualification scheduling, not production acquisition/cleanup logic.

The final provisioning probe signalled the production cancellation event **before**
launching the elevated helper for a prospective slot 4. The user approved UAC;
the helper exited with code 1 in 406 ms, before journal preparation or mutation.
The complete configuration snapshot (including ancillary fingerprint and published
layouts/owners) was identical afterward, slot 4 remained absent and no pending
journal existed. This qualifies native prewrite cooperative cancellation; it does
not claim user-declined UAC or cancellation during an in-flight PnP refresh. Earlier
native failure/restoration and successful custom creation/refresh evidence above
cover those separate listed failure and driver-refresh cases.

Together with native mismatch rejection, these results complete listed criterion 4.
All six listed criteria are now checked. At this checkpoint the issue was kept
open for the unexplained spontaneous feeder exit. The user subsequently requested
closure and merge; this reliability gap and production enablement qualification
are carried into #33, with custom logic tracked by #31. This checklist does not
declare those later milestones complete. Ordinary output/provisioning gates stay false.
Checkpoint `336aaaa` passed all 11 jobs in
[CI run 38756510846](https://github.com/opensimgear/cockpit/actions/runs/38756510846).

Raw evidence includes `vjoy-gremlin-after-inspect.json`, `busy-*.jsonl`,
`vjoy-partial-barrier-ownership-latest.json`, `vjoy-partial-events.jsonl`,
`vjoy-partial-result.json`, `vjoy-after-partial-race.json`,
`vjoy-after-gremlin-cleanup.json`, `vjoy-prewrite-cancellation.jsonl` and
`vjoy-after-cancellation-sdk.json` under `.research/review-audit/`.
