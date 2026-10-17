# Profile preflight and native admission

Slow catalogue discovery, driver inspection and isolated script compilation run
before acquiring the active coordinator or admission guards. An opaque native
preparation binds the exact saved document, local assignments, storage status
and selected game. It can be consumed once and has no IPC representation.

Final takeover acquires profile admission followed by game admission, rechecks
the current saved candidate and cancellation epoch, and then validates native
proofs and route pins. Stop, Pause and recovery cancel pending preparations.
A competing successful Apply also expires older preparation. A failed preflight
or admission keeps the previous owner renewing.

Discovery revisions remain serialized by a separate collector mutex. Discovery
does not hold the registry adoption mutex, so a current game reader can publish
its real baseline, bind read delivery and validate tokens while collection is
blocked. Metadata adoption remains atomic with overlay validation.

Focused native tests pass: 16 coordinator tests, 24 inventory/delivery tests and
two actual Manager synchronization tests. Barriers prove health, status and
emergency controls progress before slow preparation is released, without late
rearming. They also cover the profile/game lock order, competing Apply and a
saved revision changed during preparation.

The unchanged 71% per-module coverage gates pass on main:

| Module | Production lines covered |
| --- | --- |
| Coordinator | 320/333 (96.10%) |
| Recovery journal | 190/206 (92.23%) |
| One-use preparation | 80/80 (100%) |
| Native HID reader | 228/238 (95.80%) |
| Isolated script adapter | 63/84 (75%) |
| Virtual-controller adapter | 47/59 (79.66%) |
| Native game adapter | 453/503 (90.06%) |

Evidence is retained in `.research/profile-staging-coverage`. The full ten-module
native ownership/inventory/preview gate also passes (87.65–100% per module), with
source stamps and test logs in `.research/profile-staging-native10`. The pinned
official DCS-BIOS archive smoke test passed with the setup/adapters measurement.

The transition also retains the exact epoch captured before preparation, rather
than loading a potentially newer epoch after admission. The native Host checks
that fence before preparation, each activation phase and every game-baseline
wait iteration. A Stop arriving in the admission-to-prepare handoff window
therefore aborts takeover. Active health uses its existing owner/generation
fences; a successful Apply's epoch advance does not invalidate its own run.
Baseline acquisition uses cached exact route pins and actual source capture,
without starting synchronous discovery after output acquisition.

The follow-up passes 19 focused coordinator/Manager tests, including the actual
Host handoff race, all five stale phase entries and unchanged active health.
Coverage remains 96.10% for the coordinator and 100% (81/81) for native staging;
evidence and pinned archive smoke results are in
`.research/profile-transition-epoch-coverage`.

These are implementation and fixture checks; existing live script, driver and
physical-hardware qualification gates remain unchanged. Live adapter integration
continues in subsequent atomic changes.
