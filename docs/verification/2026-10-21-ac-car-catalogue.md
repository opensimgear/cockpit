# Original Assetto Corsa installed car catalogue

The running Windows development Cockpit was tested through its actual Tauri
gateway and rendered profile editor, with Assetto Corsa closed. No scanner,
setup store, catalogue, gateway or profile editor was mocked.

Both integration setup and repair discovered **178 complete installed cars** from
the selected Steam installation. Every returned folder ID and display name
matched its installed `ui/ui_car.json`. Two empty car folders were excluded.
The profile editor's vehicle selector contained all 178 friendly names with
their exact folder IDs as option values, including `lotus_elise_sc` / Lotus
Elise SC and `ks_abarth_595ss` / Abarth 595 SS.
Selecting Lotus Elise SC created and saved a vehicle tab with that exact ID
and friendly name through the actual editor and native profile store.

Throughout these checks, the current vehicle and process session remained
absent, the game stayed not running, and no verified live integration context
was created. The temporary profile was deleted after testing; pre-existing
profiles remained present and the profile runtime remained stopped.

Final local evidence:
`.research/ac-catalogue-native/1792580661548-a732a0ee/evidence.json`,
`profile-car-dropdown.png` and `profile-car-tab.png`. Screenshots were visually
inspected; selector option names and values were checked through the rendered
DOM. This final run restored the real installation's catalogue after fixture
testing.

Native regression suites passed:

- Seven scanner tests cover exact mod/variant IDs, shipped metadata with literal
  control characters, malformed content, incomplete cars, bounded work and
  redirected paths.
- Two cache tests cover persistence, installation scoping, replacement, display
  names, invalid saved data and absence of synthesized live vehicle identity.
- All 34 integration setup tests passed, including same-build repair replacing
  added/removed cars, checks avoiding rescans, publication after completion,
  cancellation preserving the previous catalogue even at the final publication
  boundary and exclusion of other Assetto games from this scanner.
- All nine vehicle observer tests and ten native context runtime tests passed,
  including immediate withdrawal of a previous installation's current car.
- All 15 related profile tab/context tests passed, including Add and Change with
  long mod names, Unicode character boundaries and trailing boundary whitespace.
  Full catalogue names remain available while saved tab labels fit the profile
  name limit. `pnpm check` reported zero errors and warnings in both applications.

The updated opt-in `tests/ui/assetto-native.mjs` acceptance passed in 36.1 seconds
through the running Cockpit. Its original-AC fixture included a second installed
mod that never published live car telemetry; both cars appeared with metadata
names. The AC, ACC and EVO authored publishers still delivered all five exact
values through applied profiles and cleared their current vehicle on exit.
Evidence: `.research/assetto-native/1792580448543/evidence.json`. These authored
publishers exercise native acquisition and profile delivery; actual game build
qualification is recorded separately in `2026-10-21-assetto-family.md`.

Installed content discovery does not establish DLC ownership. The catalogue is
refreshed by explicit original-AC setup/repair, rather than ordinary status or
telemetry checks.
