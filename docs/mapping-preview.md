# Unified profile inspection and vJoy foundations

This is an incremental implementation of [Cockpit #25](https://github.com/opensimgear/cockpit/issues/25).
The issue remains open. Cockpit authors unified Profiles, inspects applied graph
samples and vJoy, and retains optional-feature preferences and native target
setup. Live output, provisioning and qualified output recovery remain gated. Enabling vJoy is
an explicit local preference; it does not bypass the qualification gate.

## Using the current workspace

Open **Profiles** to add/open a schema-2 graph. Choose exact resource ports and
local assignments, add virtual layouts and visible transformations, and save.
Portable profiles are bounded to 512 KiB. The old Mappings editor, schema-1
desktop import, synthetic-input controls and standalone recording Start are
retired; no old configuration is migrated. The retained pure library/CLI preview
is documented in [portable mapping foundations](mapping-profiles.md).

**Inspect draft preflight** validates the captured unsaved graph without saving
or starting it. **Apply saved revision** independently checks native authority
before effects. Applied port/path inspection reads cached reducer values for
the exact running revision. Inputs and action tuples are upstream/requested
values, not successful virtual submission. Unknown, stale, unavailable and known
false/zero stay distinct. A newer draft cannot replace those samples.

Source loss releases affected contributions and cancels dependent timers. Fresh
generations and complete baselines are required before delivery; old events are
not replayed. UI refresh does not drive the reducer. Local companion preview
tests controls without acquiring a virtual device or delivering game input.

Native source assignments, backend slots and opt-ins remain local. Save/import
does not apply a graph, change driver layouts or enable optional vJoy/hiding.
Invalid local settings are preserved and reported rather than overwritten.

**Inspect vJoy** runs in a fresh process, never loads a driver DLL into the UI,
and never acquires a slot. Use driver setup/inspection under **Devices → Host
controllers**. The Windows adapter accepts the pinned, signed
2.2.2.0 x64 artifacts recorded in the M0 verification record. Other versions or
installation paths are not silently substituted. Missing optional dependencies
do not prevent graph authoring or portable previews. Native Linux/macOS physical capture
remains gated by the M1 platform qualification record.

Native target setup is independent of profile activation. The adapter validates actual button, axis,
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

The unified coordinator preflights a replacement before taking over the prior
run, obtains fresh baselines and compensates failures. It does not guess
ownership after an ambiguous acquisition acknowledgement or replay an old run.
Durable output/provisioning journals retain interrupted ownership for explicit,
compare-if-unchanged recovery. Previous-system records are cleanup-only and cannot
restart a recording, script or profile. Actual whole-app/driver failure recovery
remains unqualified; a journal is not proof that cleanup succeeded.

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
