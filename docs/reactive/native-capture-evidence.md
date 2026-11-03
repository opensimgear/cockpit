# Native capture evidence and per-port continuity

R0/R3/R4 requirements for #188. This contract guides remaining implementation;
it does not qualify full simulator freshness or routing. Hardware/simulator
acceptance stays in #199.

## Capture and admission

Keep logical profile endpoints `(resource, port)` intact. Envelope generation and
sequence govern logical reader/context continuity. Native evidence identifies an
acquisition owner, native generation, accepted nonwrapping capture sequence and
original host acquisition `Instant`. Reading a snapshot is none of those events.

Convert the original `Instant` into the immutable controller journal clock with
checked arithmetic and matching clock identity. Reject future, pre-origin and
unrepresentable evidence; never substitute receipt time, zero or an `Instant`
reconstructed from rounded `age_ms`. Timers use separate current admission time.

Mixed projections need evidence per port. A frame witness table may deduplicate
packet metadata; each contribution also names its native input key (channel,
component or BIOS address). A bare witness-index set loses that association.
Conflicting witnesses for the same contributing input cannot be silently merged.

Capture provenance and current-value validity are separate. Sampled ownship/RREF
values expire from their original capture deadlines. Latched BIOS memory and held
HID levels may have old immutable captures while a real native continuity lease
keeps them current. Reading a snapshot cannot renew that lease.

Values and contributors travel together through native projection/conversion.
Scaling preserves contributors. Magnitude, ratios and strings union their actual
dependencies and require every contributor's capture/lease policy to remain valid.
Updating X cannot renew retained Y/Z; a BIOS counter update renews its native lease
without relabeling untouched memory as newly captured. Pin validity mode and lease
membership from the trusted adapter at baseline; updates cannot weaken the mode.

## Independent family recovery

Each port needs monotonic continuity separate from its validity epoch, with
explicit baseline/update/withdraw admission. Whole-source generation replacement
is reserved for a replaced logical reader or shared aircraft/process/catalogue
prerequisite. Do not split compiler endpoints into native-family aliases.

Ownship replacement or expiry rebases/withdraws all prepared ownship ports,
including absent components as Unknown, while healthy BIOS ports remain intact.
BIOS replacement does the converse. Their availability cannot share a single
`snapshot.fresh` gate. Preparation pins family membership to reject partial
baselines that accidentally retain old values.

Install withdrawals/rebases before due timers and update occurrences. Reset only
the affected dependency closure and its tokens; nodes depending on both families
participate. Baselines establish levels without manufacturing events or one-shot
actions. Later actual updates supply occurrence authority. Per-port baseline
checks replace whole-source checks wherever independent validity matters.

## Validation and asynchronous work

Prevalidate declared ports, duplicate changes, canonical contributors, bounded
IDs/table indices, nonzero native identities and checked continuity increments.
Updates require exact admitted port continuity; native replacement needs a new
port baseline. A new envelope sequence cannot authorize an old baseline.

Check regression by native input within its producer generation, not a global
producer maximum: retained Y at sequence 8 may accompany X at sequence 12.
Equal values with new actual captures update evidence under existing trigger
semantics. Identical captures cannot masquerade as new input. Lease-only frames
need strictly advancing native observation sequence/time in the same domain;
duplicates do nothing. Their authority age is capped by both the state's
`freshness_ms` and the provider's lease duration, using checked arithmetic.
Invalid evidence cannot partially acquire authority or advance work before the
appropriate revocation.

Scripts, timers and replies retain originating contributors and continuity.
Completion/firing time is not capture time. Tickets bind invocation evidence,
run and structural dependencies; heartbeat renewal is excluded from ticket
equality. Commit-time authority still requires current matching dependencies.
A newer lease in the same latched domain may maintain that authority, but cannot
replace captures or extend sampled inputs. Lease renewal does not queue scripts,
rearm controls, reset rules or manufacture events/actions. Desktop rearming and
liveness mirroring inspect original contributors.

Bound witnesses, per-port/frame references, ID bytes, encoded bytes and retained
heap. Include evidence in native memory/queue accounting; retain no unbounded
historical registry. BIOS needs bounded current-byte capture records, overwritten
on actual writes; never discard valid latched bytes solely because their capture
is old. Account for the worst case of 65,536 distinct byte captures and copy-on-write
snapshots. Measure string/address catalogue maxima before fixing frame limits.

## Remaining integration

The [acquisition prerequisite verification](../verification/2026-11-03-native-capture-evidence.md)
records ownship receive time/counters and X-Plane component time/counters now
retained at their native owners. Their mapping integration remains below.

The [BIOS byte-capture verification](../verification/2026-11-03-bios-byte-captures.md)
records original per-byte receive positions/times through fragmented parsing and
bounded sparse memory. Counter validity observations retain their original byte
positions separately from the processed raw cursor and qualification generation.

- Native and canonical values now retain exact contributors through projection
  and cached inspection; see [paired projection verification](../verification/2026-11-03-native-value-projection.md).
  BIOS strings/masks collect their actual byte dependencies, including equal
  writes; reset/gap fences clear validity and corresponding marks.
- All four Assetto dialects now retain page-specific host acquisition evidence
  before decoding/queuing; see the [page-capture verification](../verification/2026-11-03-assetto-page-captures.md).
  Exact field/derived contributor selection is retained in the paired values. This does not resolve
  their upstream notification gate or prove simulator write time/multi-page atomicity.
- `GameSources` still needs portable captured-value envelopes and independent
  per-port family state. Reducer admission timestamps are not native proof.
- Reducer state, script/timer tickets, desktop observations and adapters need
  contributors and selective per-port baseline semantics.

Acceptance includes deferred reads, equal-value captures, unrelated updates,
mixed-age derived values, independent ownship/BIOS restart, absent components,
stale sparse bytes, old-generation replay, malformed/future witnesses, baseline
occurrence suppression and measured heap/queue bounds. Submission and actual
simulator/hardware effects remain distinct.
