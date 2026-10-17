# Unified profile native game adapters

The native profile session derives game readers and simulator commands from the
immutable compiled profile and the game-owned connection/catalogue snapshot.
Preparation opens no game transport. Acquisition validates PID, executable path,
process creation epoch, setup revision and catalogue revision; command delivery
additionally requires the applied profile context.

The adapters reuse the existing DCS-BIOS UDP transport/decoder, X-Plane source,
and Assetto Corsa Rally process-bound shared-memory source. DCS commands preserve
the captured ordered parameter contract, consume each effect sequence once, and
never replay a baseline, fallback or ambiguous send. Readiness requires observed
native counter progress, rather than process discovery or an open socket alone.
Each acquisition has its own private reader UUID, separate from the preparation
proof, so stopping an outgoing profile cannot withdraw an incoming reader.

On main, eight focused adapter tests pass. They exercise the real DCS catalogue
decoder and localhost UDP transport, exact command bytes, bounded receive queues,
stale/unknown values, context loss, immutable contracts and tuples, read-only ACR
fixtures, and distinct reader identity for a shared preparation proof. The pinned
official DCS-BIOS archive smoke test also passes.

Instrumented production coverage is 453/503 lines (90.06%) for
`profile_session_games.rs`. Evidence is in
`.research/profile-game-coverage/{native-tests.log,lines.txt,setup-summary.json,source-hashes.json}`.
The focused coverage runner was corrected to account for llvm-cov omitting its
file heading for a single requested source; the retained annotation was measured
against the unchanged source. CI keeps the same 71% per-module minimum.

These checks verify native boundaries and deterministic fixtures. They do not
qualify live simulator, virtual-controller driver, or physical-device output.
The native Host integrates the takeover and live dispatch phases separately.

The game-delivery stabilization follow-up publishes a catalogue only when its
native reader epoch, freshness or writable admission changes. Value and counter
updates remain ordinary typed frames; steady polling does not clone descriptors
or serialize retained inventory. Each active reader still validates its cached
overlay ownership every tick. A metadata failure or replacement reader disarms
the old branch even when it has no new catalogue to publish. Start checks the
actual current game baseline before admitting writable commands.

Nine adapter tests, eleven overlay tests, the native handoff cancellation test
and the pinned official archive smoke test pass on the combined sources.
Production coverage is 466/516 (90.31%) for the game adapter and 234/236 (99.15%)
for native game delivery. The full ten-module native boundary gate also passes
(87.65–100%). Evidence is in `.research/profile-game-stabilization-coverage` and
`.research/profile-game-stabilization-native`, including exact source stamps.
