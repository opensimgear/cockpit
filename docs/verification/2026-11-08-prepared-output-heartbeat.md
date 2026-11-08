# Prepared output renewal during activation

The prepared virtual feeder stays neutral while synchronous activation work
is in progress. Its worker now waits for the feeder's existing 50 ms renewal
deadline, a feeder fault, admission cancellation, phase expiry or completion.
The 250 ms lease and the pre-Start authority boundary stay with the feeder.
Prepared desktop scopes have no registered lease. Activation of those scopes
uses the original epoch and phase deadline checks without a heartbeat worker.

The focused Windows tests exercise an idle renewal window, an immediate fault
wake, epoch cancellation, expiry and work failure. The feeder fixture keeps
its neutral lease through more than one lease lifetime while activation work
is blocked. It checks that the prepared worker submitted no mapping report
and that Stop releases the slot.

## Running Windows Cockpit

The rebuilt native Cockpit ran the authored DCS UDP, browser panel and vJoy
fixture in a disposable database. Windows Raw Input received all 65 ordered
debug transitions, two transitions after the preview closed and the browser
panel press on slot 1. Stop released both buttons after 1.30 ms in this run
and the slot returned to free. The separate neutral desktop fixture verified
that Stop retired its exact helper while Cockpit stayed live. Neither fixture
used an actual simulator or a physical controller input.

Native executable SHA-256
`65828f09e640edcc5ad3aed68481bafc27783323db29ed76d5d47a9c393a7812`.
The isolated build used the Rust library format needed by the dev executable
and disabled debug symbols for Cockpit to fit the local drive. All 1,379
native source and configuration records matched the committed baseline
and the candidate after accounting for those two local build settings.
Local records are in `.research/reactive/activation-heartbeat-deadline/`
and `.research/dcs-physics-native/1794118544742/evidence.json`.

## Local checks

The Windows library suite passed 2,433 tests with 18 default ignores.
Both pinned DCS archive tests passed separately. The final assertions were
added after that suite. The rebuilt focused suite passed all 32 selected
tests and retained one existing ignore.

| Module | Production lines | LLVM functions |
| --- | ---: | ---: |
| Activation heartbeat | 95.15% | 78.12% |
| Game signals | 98.92% | 75.00% |
| Profile desktop adapter | 85.05% | 72.73% |

The coverage profile combines the full suite with the final focused run.
The pinned Rust build does not report branch coverage.
