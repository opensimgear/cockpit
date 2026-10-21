# Vehicle discovery and profile canvases

Verified in the rebuilt, running native dev Cockpit on Windows. The existing
profile editor used real gateway IPC and persisted documents; no IPC response
was intercepted or fabricated.

`tests/ui/profile-vehicles-native.mjs` added Base plus two vehicle tabs, authored
independent controller nodes, saved and reopened the profile, and checked the
automatic native conflict preview. Removing one tab deleted its nodes; Undo
restored the complete layer. Clearing the game context removed the layers, and
Undo restored the game association and both vehicle tabs. Temporary profiles
were removed after the run.

`tests/ui/profile-vehicle-switch-native.mjs` used a verified local Windows
executable named X-Plane.exe and an authored UDP RREF simulator. Before any
profile was active, native game state exposed fresh aircraft identity. Applied
topology then changed from Base + Hornet to Base + Viper, without repeated
activation for an unchanged vehicle. Stale telemetry cleared the current vehicle
and selected Base alone, including when Base owned optional game telemetry.
The extended graph reads the same native channel through Base and vehicle blocks,
with exact advertised port contracts and local pins. Inspected identity-node
samples match the authored airspeed. Fresh telemetry restores the matching
vehicle and an unconfigured aircraft retains Base. Stop prevented reactivation
while passive observation continued. The test restored installation/provider preferences and its fixture
DataRefs file, removed the temporary profile and left the session stopped;
its cleanup error list was empty.

Local screenshots and full evidence are in
`.research/profile-vehicles-native/` and
`.research/profile-vehicle-switch-native/`.

Additional checks:

- `pnpm check`: no errors or warnings.
- `pnpm ui:build`: production frontend and browser companion build successfully.
- `pnpm profile-workspace:test`: 147 passing tests.
- `pnpm navigation:test`: 42 passing tests.
- `pnpm game-launch:test`: 26 passing tests.
- `pnpm profile-vehicles:test`: 18 passing tests.
- Native DCS/export/context/vehicle tests: 133 passes and three explicit skips.
- Native profile switching: three passing regressions, including guarded retries,
  original applied revision, topology, inspector provenance and dormant optional
  telemetry during Base fallback.
- Existing native automatic-selection regressions: 17 passes.
- Portable profile schema/compiler vehicle tests: four passes.
- Portable profile unit suite: 124 passes, including shared game telemetry reads
  and retained simulator command writer conflict detection.

The existing `complete_rig` integration fixture still fails with `Unknown
persistent resource identity` after it deletes a resource from an authoritative
gateway inventory. An untouched `f9ff226` archive reproduces the same failure
before the compiler runs. That unrelated inventory fixture was left unchanged.

The live acceptance uses an authored protocol fixture. Installed DCS, Rally and
X-Plane gameplay and physical outputs have not been qualified by this run.
Assetto Corsa Rally already has native shared-memory telemetry and a carModel
identity, so the conditional fallback to original Assetto Corsa was unnecessary.
