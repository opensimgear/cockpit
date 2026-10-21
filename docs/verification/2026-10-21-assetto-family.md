# Assetto Corsa family and game artwork

The native Windows development cockpit was exercised through its actual Tauri
IPC and rendered WebView. No gateway, process discovery, mapping reader, setup
store, profile compiler or profile runtime was mocked.

## Native integration acceptance

`OSG_ASSETTO_NATIVE=1 node --test tests/ui/assetto-native.mjs` passed for original
Assetto Corsa, Assetto Corsa Competizione and Assetto Corsa EVO. The test compiled
private authored Windows publishers with each game's runtime basename and used
the production read-only mapping acquisition, process path/creation epoch,
setup verification, vehicle observer, catalogue and applied profile graph.

For each title, the actual profile reducer reported 4500 RPM, second gear,
123.5 km/h, 9000 maximum RPM and a 0.5 rev ratio through five read-only ports.
AC/ACC used their static maximum; EVO used its own namespace and layout. Parser
regressions also reject using Rally's engine limit extension for AC/ACC. Process exit
cleared the current vehicle. Private profiles and manual installation overrides
were removed/restored by the acceptance cleanup.

All seven local game images loaded. The real Games grid fit 1440 and 390 pixel
viewports, and screenshots were visually inspected. Artwork provenance is in
`public/game-art/sources.json`; the assets are bundled for offline use.

Local evidence from the final full profile-delivery run:
`.research/assetto-native/1792575785705/evidence.json` and its game/detail PNGs.
This run passed in 42.3 seconds after passive executable fingerprint caching,
setup refresh serialization and an explicit fixture readiness handshake.
An earlier full run passed in 31.1 seconds; the initial catalogue/setup-only run
took 95 seconds while repeatedly hashing the same installed executables.

## Actual original AC launch

A temporary empty matching profile launched Steam app 244210 through Cockpit.
The real `assettocorsa/AssettoCorsa.exe` menu process established game presence in
28.7 seconds. Its absence of driving telemetry did not establish integration
authority or prevent launch completion. The temporary profile was removed, the
runtime stopped, and the exact launcher process started for this check was closed.
Evidence: `.research/assetto-launch-smoke/evidence.json`.

## Regression checks

- 98 related frontend behavior and rendered-component tests passed.
- `pnpm check`: zero errors and warnings in both Svelte applications.
- `pnpm ui:build`: passed; existing large-chunk advisory remains.
- Native game, shared-memory acquisition, setup, profile delivery and automatic
  selection regressions passed. Explicit live-driving-only tests remain opt-in.
- All 30 integration setup tests passed, including deterministic overlap between
  passive refresh and a new setup request. Passive inspection cannot hide active
  setup progress, and request-owned refreshes avoid recursive admission locking.
- Cache regressions cover path/size/timestamp changes, forced hashing, bounded
  reuse and immediate preparation/admission revocation after an executable update.
- Producer checks distinguish the shared AC/ACC/Rally namespace from EVO's
  independent namespace and exclude menu launchers from mapping producers.

Authored publisher samples establish native acquisition and profile delivery,
not compatibility with a particular real driving build. Live driving in AC,
ACC and EVO and physical output devices remain unqualified by this change.
