# Native per-port acquisition admission

Incremental delivery for #188/#189/#192/#193. Native game values now enter the
portable reducer with original acquisition witnesses instead of receipt stamps.
This is a routing authority milestone; the full reactive cutover remains open.

## Behavior and bounds

The host pins the controller journal clock and complete per-port family/policy
membership before admission. Deduplicated capture and lease tables preserve exact
byte/channel identities, original producer positions and checked Instant-derived
timestamps. Foreign clocks, future or regressing captures, contradictory positions,
malformed references and partial family baselines fail before time/authority changes.

BIOS latched bytes and ownship sampled packets have independent continuity and
expiry. A genuine BIOS counter lease can sustain unchanged original bytes without
evaluating the graph, queuing callbacks or rearming rules. Port freshness caps the
provider's deadline separately; sharing a lease across ports cannot silently use
one port's shorter freshness for another. Reader replacement inherits counters,
then establishes new baselines without copying old levels. Inventory uncertainty
keeps already-numbered updates and withdrawal in one preflighted drain, preventing
sequence holes without allowing work before the loss barrier.

Retained timers, script FIFO requests/in-flight replies and output effects keep
immutable contributor bindings. Newer root values cannot renew old work. Expiry
retires an old in-flight head without blocking a valid successor. Desktop current
level/rearm evidence is stricter than causal completion evidence. Authored fallback
quality stays usable without manufacturing native neutral evidence, and Priority/
Any/All consume only the selected/decisive input proofs. Mixed baseline/update
families seed actual baseline descendants before processing real independent edges.

The actual Cockpit exposed a second coupling: passive vehicle tracking cleared the
exact aircraft on BIOS expiry and invalidated the whole profile context. Qualified,
current ownship can now independently maintain aircraft identity and inspection.
Fresh BIOS disagreement still blocks ownship projection; stale original captures
cannot supply identity. BIOS command readiness remains separate. A read-only
ownship profile cannot arm merely because unrelated BIOS is fresh.

Limits: 4,096 frames/32 MiB per mixed drain; 65,536 capture entries, 64 leases,
1,048,576 contributor references and 16 MiB per envelope; 32 MiB retained native
heap ownership including keys, values and shared proofs. Validity intervals are
bounded at 11 seconds. The Direct-only controller path avoids native-ledger cloning.
A debug-only 100-admission/10,000-fence check measured about 6.68 ms total; this is
not a physical latency measurement or a destination acceptance guarantee.

## Verification

Portable mapping: 193 tests and 35 module coverage gates pass. Controller: 147
tests and 30 module gates pass. Mapping host: 123 tests and 12 module gates pass.
The companion recording test now waits for observed output with a bounded deadline,
replacing an unreliable fixed 25 ms sleep; no production timing budget changed.
Desktop wrapper lifecycle coverage passes with dormant/stopped adapter tests that
send no native input. Real UDP BIOS expiry/recovery passes through the actual
canonical decoder and portable native admission, with ownship remaining live.

Production compilation passes. Executable SHA-256:
`be12736d8cf06a63e4f853ec0ecc5d07bc33947e2247bc1ffd53b208b428afff`.
The extended authored DCS fixture passes in the actual running Cockpit, including
counter-only lease renewal, BIOS expiry with ownship live, a new read-only run
started while BIOS is absent, BIOS recovery, pause/active pause/denial/frozen model
time, stale ownship, respawn, mission stop and process exit. It restores the original
installation list and reports no cleanup errors. The pre-fix fixture failed the
independent family/recovery checks; the failure evidence is retained.

The final native suite passes 1,919 tests with zero failures and 17 explicit
ignores (96.22 seconds, four workers). All 125 source-stamped module coverage gates pass (minimum 75.91%). All four authored Assetto variants pass native processing,
frozen expiry and recovery in the actual Cockpit (71.62 seconds), with original
installation lists restored and no cleanup errors. Actual pulse deadline, script
completion/freshness and independent baseline regressions also pass.

## Evidence and remaining work

All 962 staged Rust/SQL/manifests/locks match the tested candidate after
CRLF-to-LF normalization; unrelated working-tree changes are excluded.

Local portable evidence is under `.research/reactive/native-admission-*coverage/`.
The final native run is `native-admission-native-coverage-final/`; production/focus
logs and owned-path export proof are under `.research/reactive/`. Passing DCS
artifact: `.research/dcs-physics-native/1793687226314/evidence.json`; Assetto artifact:
`.research/assetto-capture-native/1793687459947/evidence.json`. Earlier failing
artifacts: `1793679213082`, `1793683601132`, `1793686392830` in the same folder.

This does not finish R0-R9. Remaining adapter service polling, shared-memory
notification decisions, global process/context migration, all-app read models,
UI/companion/MCP cutover and integrated qualification remain open. The existing
native live-script qualification gate remains enforced. Actual simulator effects,
physical controller preview/raw-to-virtual latency and elevated X-Plane notification
acceptance remain unqualified in #199, verified first in the progress lane. Overall
estimated work remaining is approximately 70%.
