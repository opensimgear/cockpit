# R3 telemetry quality and context transitions

## Running Cockpit

The authored DCS fixture ran on the Windows desktop and through browser-dev
using the production code at `b72c1d12`. It sent one ownship packet after
skipping a sequence and withheld the next baseline while BIOS continued.
Both clients reported `waiting` and withdrew the ownship values before the
ordinary one-second expiry. When ownship reported a different aircraft from
the fresh BIOS identity, both reported `waiting-aircraft`. BIOS values stayed
live in both cases. A fresh, matching ownship baseline restored the withdrawn
values under the same profile owner.

Both runs also passed the existing pause/resume, active pause, export denial,
frozen model time, independent ownship and BIOS expiry, respawn, active
catalogue change and process exit checks. Cleanup reported no errors.

| Transport | Fixture evidence directory under `.research/dcs-physics-native` | Binary SHA-256 |
| --- | --- | --- |
| Windows desktop | `1794169846994` | `d61daa22b7976ed20d4427fa87389efd03ca1cd16cd92052c064261e8252b2b0` |
| Browser-dev | `1794169921194` | `ad04a2348e497804ea6dd18c3df0f2d0b067bcd87b69c318f10fc56578ded1e5` |

The opt-in scenario is `tests/ui/dcs-physics-native.mjs`. The Windows runs
used private Cockpit data and an authored process, BIOS and ownship publisher.
No physical outputs were used.

## Native adapter evidence

| Integration | Software evidence |
| --- | --- |
| DCS-BIOS and managed ownship | The running fixture covers withdrawal and recovery of independent families. The native UDP regression checks sequence-gap generation replacement and the next advancing baseline. [Slow-reader and expiry evidence](2026-11-07-dcs-telemetry-backpressure.md) |
| X-Plane 11 and 12 | Native RREF tests cover malformed packets, silence and rearm, pause, replay, aircraft/clock changes and selective component expiry. Earlier authored Cockpit runs cover independent profile/inspection delivery and recovery. [Subscription leases](2026-11-02-xplane-subscription-leases.md), [profile delivery](2026-11-06-xplane-profile-service.md) |
| Assetto Corsa, Competizione, EVO and Rally | Native tests cover inactive or replay state, counter reset and replacement, reader errors and independent freshness by page. Authored Cockpit shared-memory fixtures cover expiry, rearm and graphics-only pause where the variant reports it. [Native capture evidence](2026-11-03-native-port-admission.md) |

The current Windows native binary passed 267 focused adapter and context
tests with seven opt-in tests ignored. This change adds only test and
verification coverage. The production code is the same code covered in the
[prior full local suite and coverage run](2026-11-08-telemetry-inventory-order.md).

The [scoped Windows denial run](2026-11-08-scoped-telemetry-permission.md)
discovered both X-Plane versions and all four Assetto variants while
withholding unverified telemetry. [Exact-process exit](2026-11-08-active-game-process-exit.md)
and [blocked or late native reads](2026-11-08-telemetry-inventory-order.md)
have separate software evidence. Granted Windows access, permission changes
during real sessions and actual simulator effects remain in [#199](https://github.com/opensimgear/cockpit/issues/199).
