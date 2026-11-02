# Reactive channel-core verification, 2026-11-02

R1 #190 foundation only; the milestone and parent #188 remain open. No producer,
application transport, routing or UI refresh cutover is claimed by this commit.
User-operated hardware/simulator qualification remains in #199, first in progress.

## Checks on the final source

- 13 native Rust stream tests passed on Windows, using the committed crate lock.
- Local LLVM production line coverage: codec 84.85%, fault 96.15%, journal 89.47%,
  lifecycle 95.28%; every executable module exceeds the 71% floor.
- Rust formatting and new-file length checks passed; all source/test files <500
  lines. Wire-neutral fixtures are included with the crate.
- 22 CI tool tests passed. Measured CI-core coverage: 100% lines, 82.50% branches,
  88.89% functions; changed native-plan core: 100% for all three.
- Workflow YAML contract tests passed. The new Nx native-streams marker selects
  the existing portable Rust job, which runs the coverage script on all three
  platform runners. Only Windows was executed locally.
- Timing inventory regeneration/check passed: 235 production timing sites,
  including the two new condition-variable waits. These wait for publication or
  actual caller deadlines; neither performs state-refresh polling.
- Changed-file whitespace checks passed. Existing unrelated source edits remain
  outside this atomic change.

The broader Nx plugin suite passed 11/12 cases in the existing dirty editor tree.
Its profile UI input-closure test expected SystemDevices.svelte and failed. The
same targeted test **passed** against an exported copy of committed HEAD
`1d8f555`, without the unrelated editor edits. No Nx plugin or profile UI changes
are included here, and this result is not reported as an all-suite pass.

## Conformance and review

Covered atomic subscribe/publish race with complete contiguous post-snapshot
delivery; independent fast/slow consumers; both frame and byte overflow; reserved
loss and blocked cursors; baseline recovery; old-generation publication and
resynchronization rejection; cancellation wakeups and capacity reclamation;
invalid/oversized frame/batch rejection before mutation; ordered -2/+3 relative
payloads through actual batch publication; and payload decode failure recovery.

Review found and fixed delayed decode failure overwriting a newer generation or
same-generation baseline/overflow/closed marker. A barrier-driven regression
delays decoding across all four transitions. Blocking receive now checks pending
control records before terminal closed state, preserving the closed loss marker.
Timeout overflow reports an error rather than panicking.

Native byte budgets measure encoded payloads, not full wire envelopes; transport
adapters must enforce envelope/process/client budgets separately. Service-owned
identity/quality/authority and receiver fencing of in-flight frames remain future
integration requirements. The primitive never invokes consumer code under the
producer lock or acquires sources on behalf of passive subscriptions.

The actual dev Cockpit idle baseline is documented separately in
2026-11-02-reactive-baseline.md. The new primitive is not yet connected to that
app. No zero-refresh, live routing latency, panel feedback, healthy-owner soak or
physical/simulator acceptance is claimed here.
