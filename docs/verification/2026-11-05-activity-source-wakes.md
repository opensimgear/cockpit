# Activity source wakes

Activity's writer no longer captures aggregate metadata every 750 ms. Four
retained native wake epochs select prepared source groups. Producers only signal;
capture, comparison, reconciliation and persistence run on the Activity writer.
The epoch is recorded before capture, so a racing commit remains pending.
An explicit Flush can still capture cached groups; committed-only reads cannot.

## Activity source matrix

| Group and Activity entities | Committed producer | Passive capture |
| --- | --- | --- |
| Games: game, integration, process, session, vehicle, discovery | Existing game domain/job/resource signals, including actual context invalidation | Prepared game Activity snapshot |
| Profiles: saved profile metadata | Existing profile service read-model commit observer, composed with its current observers | Saved documents; lifecycle observer's name cache |
| Devices: gateway, module, controller | Serial topology commits; workspace native session/version changes before UI encoding/publication; system inventory metadata changes, including controller and direct USB inventory | Cached topology, adapter states, USB snapshots and controller registry pages |
| Companions: pairing roster, name, instance, connected state | Hub presence and durable record commits; metadata initialization and startup hydration completion | Initialized Hub roster only |

Raw controller reports and game telemetry samples do not notify these metadata
captures. Existing profile lifecycle batches retain their separate delivery path.
Source watchers are weak and do not retain the Activity writer after shutdown.
Workspace projection or broker failures cannot suppress the underlying Activity
source notification, and unchanged publication retries do not recapture metadata.

Serial capture does not enumerate ports. Controller capture does not initialize a
worker, persist dirty metadata, or check command authority. Companion capture does
not open SQLite or initialize a Hub: canonical startup hydrates retained metadata
before registering Activity sources. Successful absence is an empty roster;
failed hydration remains unavailable. Later initialization publishes its cache
before notifying Activity. Retained metadata does not grant command authority.

Unavailable kinds retain their previous known entities and cannot acknowledge
unrelated actions. Each group acknowledges only its own available entity kinds;
the combined Devices group avoids independent module sources clearing each other.
Incomplete bounded controller paging is unavailable, never a truncated deletion.

The remaining 750 ms writer deadline retries outstanding failed persistence only;
it never captures sources. Hourly retention pruning remains an actual maintenance
deadline. This delivery does not change pending-event capacity or claim the full
R5 safety-lock, cancellation or R9 fault/load matrix is qualified.

## Verification

The isolated native candidate is based on main `2821f3d0`. The actual rebuilt
Cockpit executable has SHA-256
`06b667138defcb3eefad9feec927e4147884c14a69d1243107f6711712033045`.
The running dev frontend includes the user's preserved work in progress; this
delivery changes native code and its qualification fixtures, not frontend code.

The final clean two-worker run passes **2,311 native tests**, zero failures and
17 existing explicit ignores. All **12 changed native production-module coverage
gates** pass with unchanged source/test stamps; metadata is 100%, source captures
83.75%, writer 91.67%, game context 99.25% and gateway integration 82.23%.
All 165 controller tests and 35 controller production-module coverage gates pass;
the new pure cache module has 100% line coverage. Native regressions cover quiet
idle capture beyond the former interval, a background profile rename without a
Flush, independent source epochs, commits racing capture, unavailable retention,
scoped acknowledgements, weak observer lifetime, raw telemetry exclusion,
workspace projection failures, companion startup readiness/retained roster and
real versus duplicate game invalidation. A former gateway collector fixture now
explicitly commits topology after changing its fake provider, and asserts that
Activity performs zero native enumerations itself.

The first native run exposed that obsolete fake-provider assumption; the fixture
was corrected to commit its cache explicitly. The next four-worker run passed
2,310 tests but the existing acquisition-clock fixture returned no preparation
during setup. It passed immediately from the same compiled binary in isolation.
Static review found no causal path from the added invalidation wake to that setup
failure; the cause remains unproven. A clean two-worker full qualification retains
the original test and assertions. No product or fixture change hides that failure.

Seven actual-Cockpit fixtures pass:

- A remote authenticated WSS companion disconnect produces exactly one committed
  offline event in **17.5107 ms**. One setup Flush resets the baseline; only
  committed-only Activity queries run from disconnect through commit confirmation.
  The fixture revokes its own peer and restores its disabled listener with no
  cleanup errors. This measures one background Activity transition, not input or
  physical latency percentiles.
- Activity revision subscriptions, independent consumers, idle behavior and
  disposal pass.
- A slow controller preview resumes at current absolute state while an independent
  consumer advances.
- Workspace shared-cache subscriptions and idle behavior pass.
- Controller metadata rename/restore reaches independent consumers.
- Applied preview subscriptions fence retired owners and Stop.
- Neutral desktop lifecycle replacement/Stop retires scope subscriptions.

All 11 coverage-helper tests, Node syntax and Python compile checks pass. All 247 pinned timing sites have
exactly one disposition. Final cleanup confirms an audit-disabled runtime, no
profile owner, empty held/possible-held controls, cleanup complete, uncertainty
false and all virtual slots free. Physical input/display latency and simulator
effects remain separate acceptance under #199. CI remains deferred until all
feature implementation is complete.
