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
