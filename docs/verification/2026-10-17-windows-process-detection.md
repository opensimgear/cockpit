# Native Windows process detection

On 2026-10-17 the isolated Windows process smoke passed against the actual
`game_discovery`, `game_activation` and `xplane_installation` modules. Reproduce
with `python scripts/windows-process-smoke.py`; see the
[fixture instructions](../../scripts/fixtures/windows-process-smoke/README.md).
The runner records source and executed-binary hashes, matching source hashes
before and after execution, logs and exact process identities in a new directory.

The smoke exercises real Windows enumeration, process-image, creation-time and
liveness APIs with bounded private GUI subsystem helpers. It verifies external
start/exit, two-observation confirmation, wrong identity and stale proof
rejection, and replacement at the same executable path. An unrelated installation
with the same executable name does not match.

Two simultaneous processes at the same configured path, or at DCS's configured
`bin` and `bin-mt` paths, now fail with an explicit ambiguity. The Observer clears
its process session and authority. After one instance exits, the survivor needs
two fresh observations. The separate X-Plane exclusivity guard still rejects
simultaneous installations.

The first smoke exposed the first-match selection defect; the production fix
collects a unique match across all configured paths. Its source SHA-256 is
`cdae24e9cc7db65a46dc22e0ef13becee0c7c85532bdb4ffdd1f82f6e07efdf7`.
The fixed smoke passes all 14 recorded checks, and the native discovery, observation
and observation-runtime suites pass all 19 tests. All nine owned helper processes
were confirmed stopped. Evidence is retained under
`.research/p8-native-process-fixed-20261017/`.

No real game, GamesRuntime, profile coordinator, provider or gateway service is
started. This proves native Windows process behavior with owned fixtures; real
game launch, telemetry, controller and driver qualification remain separate.
