# Capture-only profile input learning — 2026-10-17

Learning observes one explicitly selected, current physical host HID controller.
It reuses the native controller journal and bounded movement detector, without
starting a profile, acquiring output, stopping another owner, or changing a
portable document or local assignment. OSG HID duplicates, companions, virtual
controllers and remembered/offline controls are not eligible.

The native commands are:

- `profile_input_learn_start {resourceId, generation, capabilityRevision}`
- `profile_input_learn_status {learnId}`
- `profile_input_learn_cancel {learnId}`

Replies use `state.profileLearning`, schema version 1, with the exact capture ID,
resource generation/capability revision, phase, reason, decimal-string native
timestamps, and up to 32 exact advertised port candidates. The capture expires
after 30 seconds. Status reads cached results; an independent native worker
consumes reports and revalidates the actual read token, route and catalogue.
Unknown request fields and browser samples are rejected.

Admission checks existing native profile, legacy mapping, game/integration,
virtual-controller, visibility and device-configuration ownership, pending
reservations and recovery. Start requires idle ownership; it never stops an
owner implicitly. Baseline preparation releases admission while retaining its
captured epoch. Explicit controls invalidate learning before admission and again
before the admitted effect, so blocked or late baseline work cannot revive it.
A pending Apply stays counted through staging. Status and exact-ID Cancel do not
wait for that preparation; an old cancellation cannot stop a newer capture.

The selected physical resource inspector exposes an explicit Learn button and
all returned candidates. Each choice first refreshes native expiry and checks
the selected resource generation, catalogue and HID route. Choosing adds only
that exact port contract and local expected-contract pin to the draft. It does
not create a route, Save or Apply. Source/editor changes, leaving the panel or
native output ownership clear the local capture; visible polling only reads
cached learning state. Cancel remains available during a slow Start.

| Check | Result |
| --- | --- |
| Native worker and Manager learning tests | 11 passed |
| Cached MappingHost capture-ownership guard | Passed |
| Gateway request/priority tests | 29 passed |
| Combined workspace/routing/runtime/inspector/learning SSR and actual-handler tests | 36 passed |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Browser and desktop production UI builds | Passed; desktop 245 modules |

Independent frontend gates retain 71% line, branch and function thresholds:

| Helper | Tests | Lines | Branches | Functions |
| --- | ---: | ---: | ---: | ---: |
| `profile-learning.ts` | 4 | 87.88% | 100% | 100% |
| `profile-learning-controller.ts` | 6 | 100% | 90% | 83.33% |

The five learning UI tests execute the actual Svelte handlers with the real
typed client/controller. They cover passive opening, native-only start requests,
ambiguity and exact draft pinning, expiry and changed-resource rejection,
pending-start cancellation, late-response fencing, visible polling and cleanup.
A routing SSR test verifies that only the selected assigned physical controller
offers learning. These tests provide SSR/handler evidence, not interactive
physical-controller qualification.
