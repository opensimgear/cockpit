# Actual TF-51D telemetry baseline on 13 November 2026

The operator opened a single-player TF-51D mission in the installed Steam
DCS executable. A private Windows dev Cockpit used isolated SQLite, games,
DCS-BIOS and WebView state. The native executable was the previously
qualified controller-clock build with SHA-256
`49df4141ed3bc45ff98478fa971db5c69a1779621eff93e55ae81f4099df47c0`.

## Successful live path

After the operator explicitly confirmed the simulation was unpaused,
profile activation completed. The actual `flight.ownship.inAir` value was
true in the applied profile, the scoped visible inspector and the
authenticated browser companion panel. The panel received a fresh delta
with source age 7 ms. The DCS process identity remained unchanged through
the run. No simulator packets or control inputs were manufactured.
The panel used the existing compiled timing publication. This verifies
actual simulator telemetry delivery into that publication.

The fixture stopped the temporary profile, deleted it, revoked its panel
peer and disabled its listener. Cleanup completed successfully.

## Corrected diagnosis and limits

Earlier receiver runs inside the command sandbox captured no packets.
An unsandboxed receiver captured 512 real TF-51D packets with advancing
model time and populated ownship values. The earlier captures therefore
do not establish an exporter failure.

Temporary log-only diagnostics confirmed exporter loading, UDP startup
and the first frame callback. The original managed exporter was restored
on disk and hash-verified before the successful profile run. Its original
SHA-256 is `61292175eb7a053873003382b5adb03cc01a8c87b924663f9d99cd79f0cf3cc5`.
The running DCS process still had the log-only diagnostic version loaded.
An untouched-runtime final qualification needs a subsequent DCS restart.

The initial fixture began activation 1.5 seconds after issuing a resume
cue and hit the three-second preparation deadline. Activation passed on
the same build after waiting for the operator's explicit running-state
confirmation. A reactive implementation regression has not been reproduced
by this baseline. The generic deadline may mask the more specific missing
advancing-baseline timeout. That diagnostic issue remains for investigation.

Pause/expiry/recovery, context and export-permission transitions, bidirectional
commands, destination effects and the combined soak remain open in #198/#199.
This baseline does not qualify transport latency or absence of refresh
requests across those scenarios. Eleven telemetry-model tests passed locally
before this evidence commit. No production code changed.

| Local evidence | SHA-256 |
| --- | --- |
| `.research/c193-reconnect/reader-budget-max/controller-clock-9d835a067333444f8ee9e82f13957d4b/passive-live-evidence.json` | `8349204fbd9c31a3df396b7b3eb8b2a3cf559dde17ce3a2737f6a0a91362dbc6` |
| `.research/c193-reconnect/dcs-live-preflight/real-udp-1794607518950.json` | `45d324169cb69087c605e9d0e7c5f6722bfdec10e4f8e55d943b2316abbd74fe` |

## Actual pause and recovery

An operator-controlled second session established the live baseline, then
paused and resumed the actual TF-51D mission while keeping the profile and
browser panel active. During pause the profile sample became stale, the
panel delivered a stale delta and the inspector displayed an em dash.
After resume all three returned to the same known true ownship value.
The profile owner, panel session and publication hash remained unchanged.
The DCS process identity remained unchanged. Cleanup completed successfully.

The paused and recovered panel deltas had sequences 216 and 324 on session
`a4ec3ccf-1959-420a-afd2-ca20d4008fa3`. This establishes pause withdrawal
and fresh recovery for the tested ownship channel. It does not qualify
permission/context replacement, physical output or latency.

The trace is
`.research/c193-reconnect/reader-budget-max/controller-clock-faf4b234a18647bdb535bec841bd1c16/passive-live-evidence.json`,
SHA-256 `31afa88f5d6141e85bf79a0c04593a787b236533e5782f62160ba2c8fc0ac646`.
The private evidence verifier and all eleven telemetry-model tests passed.

## Control-routing preparation remains incomplete

The installed catalogue resolves TF-51D to the P-51D command module. A
lighting test draft exposed several research-harness errors before any
cockpit command was sent. The corrected draft selects the exact right-hand
Gladiator, uses the TF-51D vehicle tab, observes the API's 64-port inspection
limit and combines physical and panel inputs through one explicit Boolean
arbitration node and one landing-light endpoint writer. Read-only compiler
inspection produced an executable plan with no error diagnostics.

With DCS confirmed running, live activation of that draft still exceeded
its preparation deadline. Physical and panel command effects remain
unqualified. Independent unsandboxed captures received 128 DCS-BIOS
datagrams on both the default and explicit sender interfaces, ruling out
the proposed default-interface explanation for this attempt. The captured
binary feed is retained for comparison with the native decoder and its
baseline requirements. No production repository code changed.

## Completed physical and browser-panel control routes

The source-admission check explained the control-profile timeout. The live
DCS-BIOS multicast sender was the local adapter `172.19.112.1`, while the
private connection used command host `127.0.0.1` and unspecified multicast
interface `0.0.0.0`. `Network::accepts_export_source` admits the command host
or an explicitly selected local multicast interface. Selecting the observed
adapter through `game_integration_settings_save` admitted the real feed.
This changed only the private test configuration. The source fence and
production acquisition implementation were preserved.

The corrected profile used the actual TF-51D tab, its P-51D catalogue alias
and the exact right-hand Gladiator main trigger `hid:0009:0001:1:0:8`.
Physical and browser inputs fed an explicit Boolean OR, a 0/1 selector and
one set-on-change writer to `command:LANDING_LIGHT:set_state`. Native
feedback `channel:LANDING_LIGHT:0` fed a comparison and the browser indicator.
The initial research graph's two writers were rejected by the compiler.
The corrected single-writer graph compiled without error diagnostics.

In actual DCS, the physical trigger produced returned light states 1 and 0.
The same authenticated browser panel then produced returned states 1 and 0.
The panel received fresh on/off feedback on one session and publication.
These samples came from DCS-BIOS, rather than native submission acknowledgements.
No simulator telemetry or UDP command receiver was substituted.

The operator initially could not identify the switch, then located callout 4
in the installed cockpit manual. A panel-only repeat held the light on for
20 seconds. The operator explicitly confirmed seeing both on and off.
Physical-trigger visual confirmation was not separately asserted. The native
returned-state proof covers that physical route. The final light state was
off, matching the recorded activation baseline. Both private profiles and
panel peers were removed, listeners disabled and owners cleared successfully.

The first routing trace and the panel-only repeat were verified separately,
so the repeat is not counted as another physical-trigger test. Both evidence
verifiers and all eleven telemetry-model tests passed before this commit.

| Local control evidence | SHA-256 |
| --- | --- |
| `.research/c193-reconnect/reader-budget-max/controller-clock-8098c6fc32db4e0d9729d746d905a13a/passive-live-evidence.json` | `644c622ed72a188e9355e6b9fc5446a4fb1a21d2a4e256c0ed4573cac462331a` |
| `.research/c193-reconnect/reader-budget-max/controller-clock-e21bac269adc4a84a85f0b3f91e2690d/passive-live-evidence.json` | `d2bfe8e4562238556d9a544e2e6847eb58ec578228acfe5d215ff4e540ca383b` |

Simultaneous virtual routing, context/export-permission transitions, the
combined soak and the untouched-runtime repeat remain open. The broader
#198/#199 matrix is not closed by these individual control checks.

## Operator-confirmed effects during separated routing, 14 November

The operator visually confirmed the TF-51D landing-light switch moved on
and off for both the right Gladiator trigger and the browser panel's
eight-second hold. DCS-BIOS returned known 1/0 states for both sequences.
The authenticated panel received fresh true/false feedback on the same
session and publication. The DCS process and profile owner stayed unchanged
through the routing phases. This completes visual confirmation of those
two DCS command effects.

The same profile also assigned the left Gladiator EVO OT L trigger to vJoy
button 1. Its observed source went false, true while the right trigger
operated DCS, then false before the panel sequence. The compiler correctly
rejected an earlier draft that sent the same physical trigger to both vJoy
and a direct simulator command; the revised graph uses separate controls.

The combined test did not finish successfully. Immediately after Stop,
its cached inventory read still reported vJoy busy. The harness asserted
before collecting the independent Raw Input receiver's final result, then
terminated that receiver in its failure cleanup. No independent vJoy
destination trace was retained. Profile/panel cleanup was also skipped by
that assertion before the private launcher terminated its owned processes.
A subsequent read-only native driver inspection found slot 1 free with no
owner. That later inspection does not establish the release time or prove
the earlier in-session cleanup completed.

The research harness now attempts receiver collection and the remaining
cleanup steps independently, and uses a direct driver inspection alongside
the cached inventory status. These changes are syntax-checked; their next
live run remains pending. No production source changed. The partial-trace
verifier and all eleven telemetry-model tests passed locally.

Trace:
`.research/c193-reconnect/reader-budget-max/controller-clock-b4d9ba9b1700453b978437776e24efdb/passive-live-evidence.json`,
SHA-256 `85f690961690d0db436e89c039328bb81fa7e41cee7e676c35dfca92d2791be3`.
The original exporter remains restored on disk; the running simulator still
has the previously documented log-only diagnostic exporter loaded. The
combined destination/cleanup proof and clean-runtime repeat remain open.
