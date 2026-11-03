# Controller catalogue subscriptions — 3 November 2026

This delivers the controller catalogue slice of #188/R5/R7. Workspace automation,
other system providers, visibility jobs and whole-app acceptance remain open.

## Behavior and capture boundary

`controllers.inventory` publishes one complete metadata catalogue from the native
registry. Discovery, disconnect, first readiness, committed rename/forget/reassign,
backend status and diagnostics trigger it. Ordinary reports do no catalogue work.
The first complete report copies only its own summary row; it does not copy other
controllers, descriptors or values. The worker merges, serializes and publishes
outside the capture mutex. Selected input streams retain native values/ranges and
their actual report sequences; catalogue sequences are deliberately zero.

Preparation leases bridge capture/registration/subscription outside the registry
mutex. Disposal, panic and shutdown release waiters and retire exact publishers.
The latest handoff holds one metadata snapshot and at most 256 readiness rows;
64 concurrent preparations are admitted. Overwritten pending states and changes
above the wire budget explicitly resynchronize. Full 256-row catalogues fit the
snapshot budget. Startup HidHide self-access evidence remains immutable and is
included without a visibility inspection.

Host Controllers uses a shared subscription and local 32-row pagination; its
250 ms `controller_list` refresh is removed. Command receipts cannot overwrite
the observed catalogue. Stale catalogue state disables metadata mutations while
the selected input subscription remains independent.

The system inventory controller reads the same cached catalogue and projects
controller changes immediately using its other prepared captures. This does not
query unrelated providers. A bounded latest projection handles storage overlap;
a late full collection cannot restore an older controller view. Correlation uses
copies so repeated projection cannot accumulate aliases or mutate source caches.
Loss, startup access warnings and duplicate identities retain cleanup guards.

## Local evidence

- Controller core: **160 tests pass**; **all 34 production-module coverage gates
  pass** (minimum 75.90%, floor 71%). The four new module gates range from 95.83%
  to 100%. Tests include blocked encoding, bounded replacement, single-row first
  readiness, metadata transactions, reassignment, retirement and panic/shutdown.
- Native app: **2,050 tests pass**, zero failures, 17 existing explicit ignores.
  Observation admission: 27 focused tests pass and 95.31% production-line coverage.
- Controller client/UI/inventory: **66 focused tests pass**. The complete Devices
  frontend group passes **181 tests** and its coverage tasks. The new client
  exceeds 96% lines/branches/functions. All 12 ownership checks pass.
- Final Svelte check: zero errors/warnings. Frontend and native builds pass.
- Actual rebuilt Cockpit: two catalogue consumers observe a real connected VKB
  controller's reversible metadata change while its independent selected-input
  stream remains ready. An instrumented run and three final runs pass. No output
  or profile is activated, and physical input latency is not measured here.
- The first startup attempt ran before navigation completed; the fixture now
  waits for the document. One initial restore wait timed out. Later runs pass;
  the fixture now records baseline/failure data and compares restoration with the
  metadata command's accepted canonical name. No definitive cause is claimed for
  that first timeout, and this is not described as an uninterrupted green run.
- Surrounding native regressions: games subscriptions, scoped baselines, pulse,
  preview sharing and physical-reader replacement/Stop pass. Script-profile Save
  failed once with the recurring `Cannot inspect persistence path.` error. This
  also occurred before this slice; **#200 tracks the unresolved defect**. A retry
  is not treated as its fix. Full R9 qualification remains open.

Executable SHA-256:
`C88893059D5C2EC80F530062426206C1DF0052BDD8B7E1DA563A0ED2103B55EE`.
Local logs and fixtures are under `.research/reactive/controller-workspace-*`.
Staged native/frontend sources are compared with the isolated tested candidate;
unrelated dirty workspace files are checked for byte-for-byte preservation.

## Remaining scope

The workspace 3-second refresh still owns connection automation and liveness.
Removing it prematurely would close Pico/STM32 connections. Its replacement needs
explicit automation ownership, passive client-scoped caches and native port/cache
notifications; moving that interval to a backend worker is not an accepted fix.
Other system/provider/visibility refreshes, companions, settings/activity, wider
adapter/routing migration, complete MCP subscriptions, integrated qualification
and final CI remain in the original issues. #199 holds user-operated acceptance
and stays first in progress. Assetto sampling remains unapproved. CI is deferred
until all features are implemented. The unsupported overall percentage estimate
has been withdrawn rather than replaced with another unsupported number.
