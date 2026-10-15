# Controller mapping previews and vJoy foundations

This is an incremental implementation of [Cockpit #25](https://github.com/opensimgear/cockpit/issues/25).
The issue remains open. Ordinary Cockpit can save, import and assign profiles,
run recording previews, inspect vJoy, and retain optional-feature preferences and
target slots. Live output, Create and Recover remain disabled. Enabling vJoy is
an explicit local preference; it does not bypass the qualification gate.

## Using the current workspace

Open **Mappings**. Create a direct mapping from a physical control or synthetic
button, axis or hat, or import a version-1 profile (maximum 65,536 UTF-8 bytes).
Choose each physical source and compatible control and save the local assignments.
Start a recording preview explicitly. Synthetic inputs have Press/Release, axis
and hat controls. The preview table retains the most recent 64 displayed reports;
native history contains 4,096 reports and exposes an explicit history-gap flag.

The capture journal contains 1,024 ordered, complete native reports. It is separate
from UI snapshots. A held momentary button must be released before its first
mapped press. Stationary axes retain their current value. Source loss releases
that source's contributions; a fresh generation and complete baseline can recover.
An unknown capture-session/history gap stops the preview and requires an explicit
restart. Old session tokens cannot inject synthetic input into a replacement run.

Profiles contain logical roles and targets. Source IDs, native control assignments,
vJoy slots and integration enablement remain local. Imports and ordinary startup
do not enable optional integrations, start previews or change driver layouts.
Optional vJoy and device hiding have separate flags, both false by default.
Invalid local settings are preserved and reported rather than overwritten.

**Inspect vJoy** runs in a fresh process, never loads a driver DLL into the UI,
and never acquires a slot. The Windows adapter accepts the pinned, signed
2.2.2.0 x64 artifacts recorded in the M0 verification record. Other versions or
installation paths are not silently substituted. Missing optional dependencies
do not prevent synthetic recording previews. Native Linux/macOS physical capture
remains gated by the M1 platform qualification record.

When a profile layout matches an inspected slot, its local slot assignment can be
saved independently of activation. The adapter validates actual button, axis,
continuous-hat and FFB capabilities before acquisition; unsupported layouts,
discrete hats, busy slots and changed axis ranges are rejected.

## Implemented foundations

- Exact vJoy V3 complete reports: 128 button bits, sixteen explicitly assigned
  axis usages and four continuous hats, including neutral and diagonal values.
- Feeder ownership with initial/stop/source fallback supplied by the mapping
  runtime, fresh session/run tokens, ordered report sequences, 50 ms renewals and
  a candidate 250 ms lease. Lease checks run independently of stdin/stdout.
- Bounded helper IPC, output acknowledgements tied to the run token, deadlines,
  subprocess liveness and termination of only the owned helper.
- A provisioning transaction for recognized non-FFB layouts and the M0 default:
  before-state journal, exclusive-creation ownership, persistent-slot commit,
  bounded rollback and preservation of unrelated/OEM/FFB settings.
  Its backend is currently exercised with fault-injection fixtures.

## Remaining #25 implementation and qualification

The production elevated Windows registry/refresh helper is integrated with an
operation-scoped cancellation event, protected journals and a transient cleanup
owner. It is a private mode of the application executable, with its own false
qualification gate. Configurable recognized non-FFB layouts are implemented;
unknown/FFB/discrete-hat layouts remain unavailable. The parent lifecycle connects ordered
capture to a fresh feeder, renews quiet sessions, stops on loss, and attempts
fallback recovery of acknowledged target ownership through a new helper. These
paths are exercised with the production reducer and injected drivers; the
application gate must remain closed until actual Windows qualification passes.

The current live lifecycle requires an explicit Stop before another live Start.
It does not guess ownership when the initial acquisition acknowledgement is lost,
and it does not automatically resume a run. Interrupted in-process recovery is
retained, and durable output/provisioning journals expose interrupted operations
for explicit recovery. Actual whole-app/driver failure recovery remains unqualified;
a journal is not proof that cleanup succeeded.

Before enabling provisioning, qualify cancellation, driver refresh, concurrent external
configuration, partial acquisition, supervisor/feeder death, lease timing under
load, total-process termination, two persistent devices, real physical button/axis/
hat routing, game binding and binding retention across restarts. A successful
recording preview or read-only SDK inspection does not establish those claims.

The current transaction deliberately preserves ambiguous orphan keys if a process
dies between exclusive key creation and flushing its ownership journal. It must
not guess ownership or remove a byte-identical key created by another configurator.

See the [verification record](verification/2026-10-14-mapping-preview-m3.md)
for actual native checks and the remaining gaps.
