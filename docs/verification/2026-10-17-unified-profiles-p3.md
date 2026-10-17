# Unified profiles P3 verification — 17 October 2026

Implements #83 under feature plan #79. Provider preferences and catalogues are
owned by game details. The unified graph owns bindings and processing; P4 supplies
the applied profile and generated live execution.

## Behavior verified

- DCS-BIOS connection, catalogue/aircraft, installer/readiness and recovery live
  in DCS game details. The standalone sidebar, settings/start surface and copied
  global integration editor are removed. Integration cards link to their game.
- Setup readiness, actual process verification and telemetry freshness are shown
  separately. Finding an executable never claims a live integration.
- Preferences use opaque optimistic revisions. Immutable record/revision captures
  prevent concurrent edits from mislabelling catalogues. Installer rechecks retain
  game-owned aircraft/connection choices and do not import the retired config.
- The native broker binds an applied owner/run, game identity, actual executable
  path/PID/creation epoch and setup/catalogue revisions. No browser API can mint
  that owner. Wrong/no game, pause, PID reuse, replacement, changed capabilities,
  malformed observations and forged tokens cannot retain authority.
- A running integration is pinned to its admitted token. Fresh evidence for a new
  same-game process cannot silently adopt the old worker. Preference saves revoke
  incompatible execution synchronously; dispatch validation observes current
  native identity and setup again.
- Closed-game authoring catalogues remain available without starting providers.
  Inventory caches are keyed by game and setup revision, preserving separate
  X-Plane versions. Catalogue presence alone supplies no delivery baseline.
- Settings visits retain the existing editor and suspend its hidden polling.
  Saving retains newer local edits; navigation never selects/applies a profile.

## Automated evidence

The Windows instrumented native suite passes **485 tests**, with three existing
live/archive tests ignored in the normal pass. The separate official pinned
DCS-BIOS archive smoke passes and verifies the archive checksum. The suite runs
serial test cases under instrumentation while retaining internal concurrency
regressions. Source hashes reject mid-measurement changes.

All six setup/context production modules exceed the unchanged 71% gate:

| Module | Coverage |
| --- | --- |
| DCS setup | 84.09% |
| Game integration setup | 87.44% |
| ACR setup | 83.22% |
| Game context broker | 98.46% |
| Game context runtime | 83.84% |
| Provider settings/catalogues | 97.62% |

The inventory source suite passes 17 tests. Provider/navigation/render checks pass
15 tests, with 91.67% helper lines and 100% branches/functions. The existing 23
game client tests pass. Main/browser Svelte checks report zero errors/warnings;
browser and desktop UI builds pass. Render tests execute actual Svelte server
components; an interactive browser was unavailable in this session.

## Boundaries

P4 has not yet installed live ProfileSession ownership or compiled adapters, so
integration execution remains dormant without that native owner. This milestone
does not claim actual simulator, device, Windows driver or mobile acceptance.
Existing #50–#54 and #66/#67 remain separate qualification work. Linux/macOS native
activation gates remain in force. Cross-platform CI is recorded in PR #108.
