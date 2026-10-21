# Expanded telemetry and common channels

The seven game integrations expose typed telemetry through the game resource
catalogues. Common quantities use the `opensimgear.telemetry` version 1 channel
schema in both passive inspection and profile processing. The implementation and
coverage limits are described in [Game telemetry](../telemetry.md).

## Native cockpit acceptance

The expanded Assetto acceptance runs against the actual Windows development
Cockpit through its native Tauri gateway. It creates temporary installation
overrides, verified fixture processes, authored shared-memory pages, and profiles
with identity processing nodes. It does not mock the gateway, resource inventory,
profile compiler, game reader, or processing runtime, and it connects no physical
outputs.

```powershell
$env:OSG_ASSETTO_TELEMETRY_NATIVE='1'
$env:OSG_NATIVE_CDP_URL='http://127.0.0.1:9224'
node --test tests/ui/assetto-telemetry-native.mjs
```

The final run passed for Assetto Corsa, Competizione, and EVO in 51.2 seconds.
It verified native physics fields and shared channel contracts, values through
the applied processing graph, unbound fields in passive inspection, category
pages, ACC's explicitly unused fields, independent invalid-component handling,
pause and stale withdrawal, recovery, and process exit. In particular, an invalid
g-vector component withdraws the shared magnitude without discarding unrelated
physics values. Evidence is retained locally at
`.research/assetto-telemetry-native/1792584950160/evidence.json`.

These expanded-field runs use authored protocol fixtures. The previous real
AC/ACC/EVO driving qualification of the five original channels is recorded in
[Assetto family verification](2026-10-21-assetto-family.md). The expanded fixture
results do not qualify every new field against a physical driving session.

The DCS acceptance also passed against the final rebuilt native Cockpit in
74.6 seconds:

```powershell
$env:OSG_DCS_PHYSICS_NATIVE='1'
node --test tests/ui/dcs-physics-native.mjs
```

It copies an installed DCS-BIOS catalogue into a private user folder and verifies
actual managed Setup and Repair, generated exporter/catalogue hashes, the unique
loader, passive unbound BIOS and physics fields, and selected native/canonical
fields through identity processing. Zero and false remain known. Pause, active
pause, permission loss, frozen model time, silent UDP, ownship replacement,
mission stop, and process exit withdraw physics values; fresh BIOS gauges remain
known independently. Cleanup restored the previous settings with no errors.
Evidence: `.research/dcs-physics-native/1792585556353/evidence.json`.

Both X-Plane versions passed against the running native Cockpit in a combined
218.6 seconds:

```powershell
$env:OSG_XPLANE_TELEMETRY_NATIVE='1'
node --test tests/ui/xplane-telemetry-native.mjs
```

Each fixture publishes the full 63,607-port installed numeric SDK catalogue.
Selected native scalars, array components, RPM, ground speed, true airspeed,
altitude and g magnitude reach the actual processing graph. Passive page
subscriptions, replacement/unsubscription, missing-field isolation, pause,
frozen and silent UDP, recovery and exit are also checked. Request pacing stays
within 100 requests per 95 ms. Advancing baselines completed in 277 ms for
X-Plane 11 and 211 ms for X-Plane 12, within the unchanged three-second deadline.
The publishers are authored RREF fixtures, not live simulator qualification.
Cleanup removed the fixture processes, sockets and profiles, removed the
previously absent X-Plane 11 provider, and restored the actual X-Plane 12
connection. Evidence:
`.research/xplane-telemetry-native/1792587260971/evidence.json`.

The final rendered native UI acceptance passed in 1.05 seconds:

```powershell
$env:OSG_GAME_TELEMETRY_NATIVE='1'
pnpm game-telemetry:native:ui
```

It opens Debug from the actual profile list, checks all seven game choices,
canonical RPM units, grouped/paged EVO physics and explicit unknown values, opens
both preserved Debug sections, enters the real Default editor to verify that no
Debug button is present, then returns to the list and reopens Debug. The screenshot
was visually reviewed for layout, theme and overflow. Evidence and screenshot:
`.research/game-telemetry-native-ui/1792587840527/`.

## Actual integration repair

Native setup replaced the authored AC/ACC/EVO records with the real installed
executable hashes and restored AC's complete 178-car scan. Those builds remain
configured but unverified until their next live driving verification; fixture
qualification was not carried over to real executables.

DCS Setup installed the actual managed ownship exporter and one loader in
`Saved Games/DCS`. Its existing Export.lua content, connection settings and MH-6J
preference were preserved. Native inspection reports Ready, 101 physics fields
and port 7781, and the generated source/catalogue digests match the installed
files. The profile runtime was stopped and workspace fixture installation
overrides were cleared. Local evidence:
`.research/final-native-integration-repair/1792587048513/evidence.json`.

## Regression contracts

The combined frontend regression run passed 179 tests, covering the telemetry
client/controller, rendered inspector, list-only Debug entry, preserved profile
Debug tools, inventory bounds, gateway request queues, and profile workflows.
`pnpm check` reported zero errors and zero warnings in Cockpit and the browser
companion.

The final full native library suite passed 1,068 tests with zero failures and
eight opt-in tests ignored (`cargo test --lib -- --test-threads=8`, 357 seconds).
The final inspection correction also passed both focused regressions: mixed
supported/unsupported X-Plane pages retain all rows while subscribing only to
supported fields; an unsupported-only page clears the inspection subscription.

Shared-channel tests compare engine RPM and ground speed across adapters, retain
legacy channel contracts, require complete g vectors, and use the same selected
X-Plane conversion in processing as in passive inspection. RPM percentage remains
distinct from actual RPM, and ACC fuel mass remains distinct from fuel volume.

Large-catalogue regressions cover 65,536-port publication, aggregate inventory
bounds, cached contract validation, transactional copy-on-write, and cache
invalidation. Persistence tests block the storage writer while acquisition and
capture continue, and fence raced changes and stale writes. Inspection has a
separate read-only admission path, so it does not wait behind profile operations.

The installed 63,607-channel catalogue exposed a live activation bottleneck:
adding a 15-port delivery to its warm resource took 3.5 seconds. Immutable
per-sighting validation, hashes and exact serialized byte counts reduced that
reconciliation to 43 ms, with the first observed-port projection at 157 ms and a
repeat at 31 ms in the focused benchmark. The original three-second advancing
baseline deadline and all source freshness limits remain unchanged. An additional
regression repeatedly binds selected ports against a 32,769-port admitted
catalogue and revokes the old delivery after a changed contract, even when the
provider reuses its declared setup/catalogue revisions.

DCS regressions hold the BIOS counter while ownship physics advances, freezes,
resumes, or changes session. They verify independent physics withdrawal and
baselines, continued BIOS authority, visible bounded socket errors, and Unknown
for a missing component of an otherwise live frame. Setup, Repair, removal,
rollback, and old BIOS-only setup migration include the managed physics hook.

## Practical limits

Rally's unqualified extended layout remains unavailable. X-Plane publishes the
installed numeric DataRef catalogue and subscribes to selected processing inputs
and the visible inspection page; byte/data blocks, unspecified arrays, and giant
lookup tables are explicitly unavailable. DCS-BIOS publishes the current loaded
aircraft's installed exports, supplemented by 101 documented ownship fields.
Missing SDK units and unqualified coordinate frames retain their native meaning.

No motor-control backend or hardware force-feedback qualification is implied by
these telemetry and processing checks.

## Dedicated Debug navigation

A follow-up moves all three diagnostic views into the main navigation's Debug
section and removes the Profiles entry button. Saved-profile preflight uses its
own read-only context while runtime status comes from the cockpit's shared
controller. Leaving Debug disposes the inspection controllers and polling.

The updated native UI acceptance passed in 2.24 seconds against the running
development Cockpit. It verifies the selected sidebar entry, incoming telemetry,
native saved-profile preflight, runtime/recovery tab, absence of Debug buttons in
the profile list and editor, unmount on departure, and Back/Forward restoration.
The screenshot was visually reviewed. Evidence:
`.research/game-telemetry-native-ui/1792588307153/`.

All 81 focused rendered/navigation tests passed, including the unsaved-profile
navigation guard. `pnpm check` reported zero errors and zero warnings in both
Cockpit and the browser companion.
