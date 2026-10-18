# Devices final-scope acceptance

This supersedes the visible counts in the earlier
[inventory verification](2026-10-18-unified-devices.md). It incorporates the user's
later requests in **Show active integrations only**:

- Show integrations with bindings in the immutable applied graph, even while
  telemetry is waiting; catalogue support or data reception alone is insufficient.
- Fold assigned panels into their paired companion by name. Keep unpaired USB
  setup records and pending pairing requests in Companions.
- Preserve the compact tile design, distinct device-type icons, simple transport
  badges, combined count row and absence of chevrons/status text. Accessible tile
  names and tooltips retain observed state; confirmed offline tiles are muted.

Hardware, gateways, virtual devices and paired companions need no profile for
discovery. Profile authoring, activation and signal-routing visualization remain
outside feature #78. Read-only applied metadata only determines the requested
integration visibility.

## Running dev Cockpit workflows

`pnpm devices:ui` passed in 35.03 seconds against the running dev frontend at
`http://127.0.0.1:1420`, using deterministic native IPC replies. Its initial
14 visible components cover two gateways, four physical OSG components, three
third-party controllers, two virtual targets, one bound integration and two paired
companions. The inactive integration and separate assigned panel remain absent.

The complete workflow checks shared IDs, type-aware counts, List/Topology parity,
pan/zoom/Fit, combined filters, dedicated details/Back, selection restoration,
automatic updates without Refresh buttons, stable tile styling, muted offline
state and accessible Connected+Hidden state. It verifies bound-but-not-receiving
integration visibility, removal of bindings, stopped applied ownership and
restoration; pending pairing does not appear or acquire approval. It also tests
provider loss/recovery, independent USB/CAN identity, retained companion endpoints,
gateway Disconnect/Connect and module configuration round-trip, virtual metadata
cleanup, concurrent reconnect, partial bulk failure, reference disclosure,
restart/stale-snapshot suppression, genuine rediscovery and unavailable-gateway
cleanup preserving independent USB routes. Unexpected IPC and page errors are empty.

`pnpm devices:native:ui` passed in 1.68 seconds against the actual native bridge at
`http://127.0.0.1:5175`, without IPC interception. It observed seven controller
records, three vJoy targets and three paired companions. Two unmatched retained
PnP rules remain separate physical observations. List and Topology contain the
same 15 IDs: nine third-party physical observations, three virtual devices and
three companions. The registry retains 22 underlying observations, including four
unbound catalogue integrations and three assigned instances; those seven are
intentionally folded/suppressed by the requested presentation rules.

Native inspection, mode switching and details/Back issued no configuration,
visibility, acquisition or cleanup mutations. Screenshots were opened for visual
review. The separate desktop's ownership diagnostic in the browser bridge is
expected while both processes run; it does not establish an active profile.

Evidence is local and ignored; SHA-256:

| Evidence | Digest |
| --- | --- |
| `.research/feature78-final/current-fixture-ui/evidence.json` | `a1387f975cfbb7b7d95f56a3dac709fa7576af0ca40e09a9880f9489f1dad3f6` |
| `.research/feature78-final/current-native-ui/evidence.json` | `33e2681bd6ceeb91cc7271cd22d2ab939f8ba5f9738cb35a7cbe3a4af0e2e765` |

## Other validation and qualification limits

- Shared inventory/provider/topology coverage gates pass: 16 contract, 42 provider
  and five geometry tests. Each module exceeds the unchanged 71% thresholds.
- Type checks report zero errors/warnings; production UI build passes with its
  existing chunk-size and mixed-import warnings.
- Existing transport regressions, 88 profile-workspace UI tests and 11 game-entry
  regressions pass. Full native library result: 826 passed, zero failed, six
  existing ignored tests (50.69 seconds).
- [HidHide startup access](2026-10-18-hidhide-self-access.md) has separate full-policy
  before/after and idempotent restart evidence; periodic inventory stays read-only.
- Real standalone/output-only OSG USB, CAN gateways, dual USB/CAN hardware and
  game/output qualification remain fixture/native-protocol coverage where the
  physical rig is absent. Existing hardware qualification issues/gates remain open.

Final CI conclusions and delivery revisions belong in the feature/story delivery
records. Local checks do not imply a still-running or superseded CI run passed.
