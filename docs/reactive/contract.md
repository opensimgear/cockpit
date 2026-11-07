# Reactive stream contract (R0)

Work for #188 / #189. This contract is a qualification target, not a claim that
current transports implement it. Hardware acceptance is tracked in #199.
The [native capture evidence contract](native-capture-evidence.md) specifies
per-port provenance, independent family recovery and remaining integration.

## Authority and identity

One service owns acquisition and its committed read model. Observation never
activates a profile, collects mutable inventory, renews source evidence or gains
command authority. UI, browser bridge, panel and MCP subscriptions read that same
owner. Source identity includes adapter, device/process, session and generation;
route, boot, configuration, publication and owner fences remain mandatory wherever
the source supports them. No old-generation callback may update a new session.

Each envelope carries topic, source identity, generation, sequence, revision,
capture timestamp and clock identity, continuity, quality, and typed payload.
Sequence and revision cross JSON as unsigned decimal strings. Native integers
outside JavaScript's safe range also use decimal strings with signedness/width;
never normalize a physical axis to conceal its observed range. Numeric domain
metadata is immutable per descriptor generation and is fetched once.

Connection, continuity, freshness and command authorization are separate fields.
An unchanged control is connected without new reports. Only captured evidence
advances freshness; receive/render time cannot. Telemetry expiry withdraws its
affected values without inventing a transport disconnect.

## Snapshot and delivery

Under one owner lock, register the subscriber and capture the snapshot at revision
R. Deliver the snapshot before queued updates strictly after R. Snapshot includes
current levels and metadata, never historical occurrences. Mutation commits and
worker completions increment revision before publishing. Query replacement and
transport reconnect dispose the old subscription and require a new boundary.

State topics may replace pending absolute values only when the complete atomic
report/group is retained and no ordered transition is crossed. Button edges,
relative deltas, encoder steps, commands, lifecycle and loss records are ordered.
Baselines restore levels and cannot trigger commands. Existing independent-source
batch admission installs every revocation before advancing any mapping timer;
see ../profile-source-batches.md. Do not sort losses and then admit individually.

Each subscriber has a bounded queue and byte budget. Overflow invalidates the
cursor, clears pending effects and delivers an explicit loss/resync result through
a reserved control slot. Further data is withheld until a fresh atomic baseline.
Cursor loss, sequence gaps and generation replacement have the same recovery
requirement. No automatic command replay, including after uncertain submission.

Subscriptions are explicitly closed on unmount, parameter/window/session change
and transport close. Shared frontend subscriptions are reference-counted by exact
topic/query identity. Registering/disposal cannot block capture, Stop or renewal.

## Commands and clocks

Commands retain existing admission, compensation and neutralization. Track desired,
submitted, acknowledged, uncertain and telemetry-confirmed effects separately.
Output feedback cannot be recaptured as a fresh input occurrence. A slow destination
gets an independent bounded worker, never execution inside a producer callback.

Wake reducers for source batches, committed authority/config changes, completions
or the next actual authored deadline. Lease renewal, watchdogs, retry backoff,
freshness expiry, debounce, request deadlines and animations keep named owners.
Timers do not query state. One dirty UI frame may schedule presentation; rendering
cannot manufacture input samples or renew source freshness.

## Initial resource budgets

These are proposed implementation ceilings; live R0 evidence must confirm them.
Existing tighter protocol limits continue to apply.

| Resource | Ceiling / behavior |
| --- | --- |
| Envelope | 64 KiB; reject before allocation/admission |
| Atomic source drain | 4,096 frames, matching existing mapping admission |
| Ordered subscriber | 1,024 frames AND 1 MiB; overflow faults/resyncs |
| Absolute-state subscriber | 256 pending source/group keys AND 1 MiB |
| Observation subscriptions | 64 per process, 16 per external client |
| In-flight observation delivery | One per subscriber; no unbounded tasks |
| Snapshot | 1 MiB; larger catalogues use immutable revision-pinned pages |
| Sink processing | Independent worker; existing Stop/lease bounds preserved |

Target representative workload: simultaneous 125 Hz and 1 kHz synthetic sources,
ordered button/encoder bursts, simulator telemetry, two panel consumers, one UI
and one MCP observer, including one deliberately stalled consumer. Synthetic rates
do not assert hardware capability. Record actual source cadence separately.

Capture-to-destination acceptance p95 <=20 ms / p99 <=50 ms; foreground UI p95
<=100 ms; LAN panel p95 <=150 ms, excluding measured upstream/transport cadence.
Trace capture, enqueue, dequeue/reduce, dispatch, native acceptance and presentation
with source/generation/sequence correlation. Report p50/p95/p99, event/loss counts,
queue/byte peaks, owner continuity and minimum lease margin. Compare clocks only
within a monotonic domain or with measured clock correlation and uncertainty.

## Gates

R0 remains open until the acquisition inventory is complete, conformance fixtures
are exercised, and real Cockpit baseline traces confirm/revise these budgets.
One native sampler per active Assetto game is approved for the four Assetto
shared-memory adapters. Preserve packet identities, mapping generations, capture
timestamps and freshness. Publish detected changes to the shared stream for
profiles, inspector and MCP consumers. Controller input is outside the exception.

One shared Windows check is also approved for external vJoy ownership while
Devices status is visible. This check supplies status and owner PID to the
inventory stream. Controller capture and virtual dispatch keep their own
native paths. [vJoy decision](https://github.com/opensimgear/cockpit/issues/189#issuecomment-6704680610)

Other sampled acquisition requires a producer bridge or an explicit
feature-level decision. The shared channel contract applies to each adapter.
[Assetto decision](https://github.com/opensimgear/cockpit/issues/188#issuecomment-6646558859)
