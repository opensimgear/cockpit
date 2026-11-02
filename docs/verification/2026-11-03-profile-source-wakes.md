# Source-scoped profile input wakes — 2026-11-03

R2 #191 / R4 #193 / parent #188 remain in progress. Local date is Europe/Berlin;
the final running-Cockpit checks completed around 23:02 UTC on November 2.

## Delivered behavior

The global controller-to-profile wake callback is removed. Each native HID input
reader owns a bounded wake lease for its exact physical source IDs. A committed
report, baseline, gap or disconnect from a selected source still wakes the profile
worker immediately through its atomic epoch/unpark mechanism. Reports from
unassigned controllers do not notify it merely because capture is running.

Registration occurs under the controller registry mutex before baseline capture.
An intervening report is either included in that baseline or follows its cursor;
it cannot fall between baseline capture and wake registration. The final Start
reader is acquired before replacing the initial Baseline reader, so successful
replacement has no unobserved interval. Failed capture drops its lease. Native
neutralization takes the input reader before potentially lengthy cleanup; later
owner disposal is a backstop. Lease destruction takes no registry or host lock.

The controller journal remains global and bounded to 1,024 reports. Input readers
acknowledge the cursor of copied snapshots/pages. Evicting history that a live
reader has not copied wakes that reader even if the evicted input was unassigned;
existing explicit gap/baseline recovery remains enforced. Evicting already-copied
history does not wake it. Cursor exhaustion wakes all live readers and never
wraps identity. This is conservative global-history loss handling, not a claim
that missing unrelated reports prove the selected physical device disconnected.

There are at most 64 live leases, each selecting 1–16 distinct known controllers.
The journal retains weak lease references and speculative registry clones retain
none. Callbacks only signal; they cannot collect state or acquire routing locks.
No subscription, metadata read or acknowledgement renews a captured input sample.

## Verification

- Isolated controller suite: **142 passed**, zero failed; all production module
  line gates pass 71%. Journal/watches **100%**, service **81.23%**. Checks include
  source/reason filtering, overlapping leases, disposal under the registry lock,
  snapshot/report boundaries, acknowledged versus unread eviction, exhaustion,
  validation, lease capacity and speculative clone isolation.
- Focused native input reader tests: **12 passed**, including the registration
  race, replacement overlap, page acknowledgements, unread overflow, failed
  baseline cleanup and unused-source plans.
- Full isolated source-stamped native suite: **1,820 passed**, zero failed,
  17 explicit helper/opt-in checks ignored; 96.58 seconds with four workers.
- Changed production coverage: input readers **87.46%**, activation phases
  **83.42%**, native host **80.32%**, gateway **84.05%**. Integration coverage
  harness regressions: nine passed. The existing large input-reader test module
  is extracted into focused files; its production file is below 500 lines.
- Native production executable builds. SHA-256:
  `4cf81bb4e8220a38a664db3df45f08fd56e640f8c8e3677c2db83a905cc8372e`.
- Actual Windows Cockpit: the opt-in physical-input fixture creates a supported
  profile containing one exact observed HID integer port and contract assignment,
  routes it through an identity node with its output unbound, and applies it.
  The processed output is known and within the declared native integer range.
  Reapplying the same revision acquires a new exact owner and admits its baseline.
  Stop clears the owner and rejects late old-owner inspection. No preview
  subscription is opened by the fixture; no external sink, virtual controller,
  desktop output or simulator effect is authored. The fixture is deleted afterward.
- Both actual authored pulse and internal script completion/expiry fixtures pass
  again after the global physical-report wake was removed. Their deadlines cannot
  depend on unrelated physical reports accidentally waking the worker.

Run the opt-in physical source check against the native dev Cockpit:

```powershell
$env:OSG_PROFILE_SOURCE_WAKES_NATIVE = '1'
node --test tests/ui/profile-source-wakes-native.mjs
```

The source fixture initially exposed incorrect assumptions in its own authoring:
resources start without selected ports, HID axes have integer contracts, and
identity is a direct operation rather than a built-in rule. Final evidence uses
the supported observed-port assignment and canonical identity shape. Rejected
drafts did not activate an owner or external output.

Local evidence: `.research/reactive/r4-source-wakes-*`, controller coverage in
`.research/controller-coverage/`, and the final physical fixture under
`.research/reactive/profile-source-wakes-native/1793660552749/`.

## Remaining scope

External-adapter service polling, automatic selection/read models, other producer
notifications and the complete UI/MCP cutover remain open. Actual assigned reports
still preserve their existing admission semantics; this milestone does not
eliminate all repeated graph work or qualify idle-zero-work for the full app.
It does not measure raw capture, wake precision, virtual receipt or display
latency. Native capture and admission clocks still need explicit separation.
Physical sweeps, destination receipt, simulator effects and hardware qualification
remain in #199, first in the progress lane.
