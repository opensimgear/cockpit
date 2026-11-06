# Controller preview catches up after a slow receipt

The read-only controller preview now coalesces unread ordinary, ready-state
reports into the latest complete absolute snapshot. Routing's typed capture
journal still retains each button transition and report in order. Partial and
complete baselines, metadata, disconnect, generation replacement and explicit
resynchronization keep their existing publication paths. This adds no timer.

The frontend validates each snapshot and reuses unchanged rows and descriptors,
including the complete controls array when values are unchanged. This avoids
redrawing every control merely because another full state arrived.

Local evidence, 2026-11-05:

- The old running native Cockpit reproduced stale replay on an attached
  137-control VKB Gladiator. With the first receipt held for 750 ms, capture
  advanced from report 12948 to 12958, but the next preview was report 12949.
  An independent preview received eleven frames during that interval.
- The same fixture on the rebuilt Cockpit resumed at report 58 when capture
  was at report 58. It delivered a complete same-generation snapshot. The
  independent preview again received eleven frames. This proves backlog
  recovery for the tested workload, not physical motion-to-photon percentiles.
- All 164 controller tests pass, including slow/fast readers, every ordered
  routing button edge, atomic partial baselines and disconnect/replacement.
  The changed native publisher has 124/134 production lines covered (92.54%).
- All 2,286 native tests pass, with 17 existing explicit ignores. All 59 related
  frontend tests pass. The controller read model has 99.59% line, 96.57% branch
  and 100% function coverage. `npm run check` passes without diagnostics.
- Three actual running-Cockpit regressions pass: shared physical preview
  subscribers/reopen without inspection queries; physical activation,
  replacement, quiet liveness and Stop with previews closed; real vJoy Raw Input
  transitions plus independent MCP readers and Stop/retirement. Temporary
  profiles/readers are cleaned up and the virtual slot returns free.
- The isolated candidate's 1,158 tracked Rust/manifests/locks differ from the
  committed source only in the two intended edited controller files. The new
  test module matches. Changed test files remain below 500 lines.

Two initial unit assertions incorrectly expected 64 rather than 65 journal
events (the baseline is retained), and an invented disconnect generation loss.
They were corrected to assert the existing protocol; the full 164-test rerun
passes. The first restart could not replace the locked old executable, so its
failed live result is excluded. The successful restart waits for exact process
exit and verifies the copy hash before testing. Native SHA-256:
`20bac815535c04094d3572dad86a6d9c6a0ec06e5941bab463d7c60786fbc65b`.

Evidence is under `.research/reactive/controller-latest-*`, including the
before/after JSON, runtime identity, coverage and full-suite logs. The durable
test is `tests/ui/controller-latest-state-native.mjs`, enabled explicitly with
`OSG_CONTROLLER_LATEST_NATIVE=1` and `OSG_NATIVE_CDP_URL`.

Desktop journal/IPC isolation remains separate software work under #193. Moving
it behind a queue must preserve ACK-dependent tap cancellation, count queue age
against deadlines and fence revoked work before native submission. Merely
predeclaring every possible profile key would broaden recovery releases to
controls the run never emitted and is not an equivalent optimization.
Full-app reactive qualification remains open under #188/#198. Physical latency
and actual simulator acceptance remain #199, verified first in Progress. CI
remains deferred until implementation is complete, as requested.

## Shared capture with an applied profile

A second running-Cockpit test on 2026-11-06 paired the production preview subscriber
with an output-free applied profile on the same 137-control VKBsim Gladiator EVO R.
Preview alone left the profile stopped. Both consumers reported native axis value
3493 and capture time 540120239 microseconds from generation 1. The preview
identified report 6746. The comparison uses the profile input endpoint because
the derived mirror output has a graph evaluation timestamp.

After unsubscribe, the preview received no more frames. The profile kept a
known output across a 1,750 ms quiet interval, survived same-profile replacement
with a new owner, and rejected inspection after Stop. The fixture removed its
temporary profile. No desktop, virtual or physical output was configured.

All five running checks passed in one serial run. They covered the shared capture
test, passive subscription sharing and reopening, delayed preview recovery, and
passive USB and CAN admission. Module capture while routing, physical controller
latency and actual destination receipt remain open.

Tested executable SHA-256:
`bfc0f71a23b850aff4901d0d4b26c123ea7c60fc99ccfe91d43f0d3e8c1f45ea`.
Native source commit: `e12557aa0a01a23e913c4a26b68a7aae2bad35e8`.
Evidence: `.research/reactive/profile-source-wakes-native/1793930945405/evidence.json`.
