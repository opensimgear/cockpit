# Unified profiles P0 implementation evidence

Scope: [P0 #80](https://github.com/opensimgear/cockpit/issues/80) of the
[unified profiles plan #79](https://github.com/opensimgear/cockpit/issues/79).
Implemented from `origin/main` at `78328a0` on the isolated
`feat/unified-profiles-p0` branch. This record accompanies the implementation.

## Delivered boundary

- Strict shared portable graph contracts and Default/Blank/Mixed JSON fixtures.
- Protected editable Default, independent named documents, optimistic revisions,
  separate local resource assignments and a fresh `profiles-v1` library.
- Bounded atomic persistence, corrupt-source preservation and explicit reset.
- Separate editor drafts, selected profile metadata, immutable captured revisions,
  manual/automatic mode and pause/fault state. Manual selection pins manual mode.
- Native/browser IPC and a typed client. Profile requests have separate bounds and
  ordering; ordinary gateway requests retain their previous bounds.
- No legacy document migration or hardware activation. `profile_apply` returns a
  P4-unavailable error; captured snapshots are data, not evidence of executed
  device effects. Discovery/compiler/runtime/UI milestones remain separate.

The mixed fixture includes third-party A/B into a virtual output, CAN and direct
USB logical sources, a second virtual output, explicit event-to-command routing,
game telemetry through visible conversion/threshold nodes, companion feedback and
a module action. It is a contract fixture, not an executed simulator scenario.

## Local checks

| Check | Result |
| --- | --- |
| Native profile crate tests | 29 passed |
| Frontend profile tests | 16 passed |
| Frontend profile coverage | 100% lines, 99.15% branches, 100% functions |
| Frontend and browser type checks | Zero errors and warnings |
| Transport regression suite | 173 passed |
| Browser companion and Cockpit UI builds | Passed; existing bundle-size/import warnings remain |
| Windows desktop library tests | 375 passed, 3 existing live-hardware/game tests ignored |
| Native browser-service framing | 1 passed |

The restricted Windows sandbox cannot supply the DPAPI context required by two
existing companion tests. The full native library rerun with the Windows user
context passed. This does not qualify physical controller, vJoy, HidHide or game
execution.

Native coverage measured by `scripts/profiles-coverage.py`:

| Production module | Covered lines | Coverage |
| --- | --- | --- |
| library constructors | 8 / 8 | 100.00% |
| model | 43 / 44 | 97.73% |
| validation | 320 / 354 | 90.40% |
| session | 76 / 76 | 100.00% |
| store | 145 / 152 | 95.39% |
| service | 248 / 251 | 98.80% |

Each module exceeds the 71% production line gate. Shared fixtures, strict
unknown-field rejection, reference/type validation, limits, corruption, failed
atomic replacement, copy/import isolation, restart, manual selection, Pause,
faults and immutable snapshots are exercised. Frontend regressions also cover
editing during an in-flight save and reopening/switching editors before a reply.

CI adds the native profile coverage job on Windows, Linux and macOS, plus the
frontend profile coverage gate. Local evidence above is Windows evidence; results
from other platforms must be recorded from their own CI/native runs.
