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

## Actual original AC driving

The installed `assettocorsa/acs.exe` was driven in offline Practice with the
Lotus Elise SC (`lotus_elise_sc`) at Magione. Its native pages reported game
version **1.16.4**, shared-memory version **1.7**, live status `2`, and advancing
original physics packet IDs. The running development Cockpit verified that real
process and published its five read-only channels through an applied profile;
no authored publisher supplied these samples.

The final driving capture contained **255 samples**, each with all five channels
known: RPM **850–8123**, gears **0–4**, speed up to **143.579 km/h**, a constant
published maximum of **8500 RPM**, and rev ratio **0.1–0.955647**. Pause withdrew
all channels in six observations; resume restored five fresh observations with
changing RPM and speed. Exiting the driving runtime withdrew all channels in
three observations. The AC menu remained detected, while its exited `acs.exe`
runtime no longer retained verified telemetry authority.

Native Cockpit evidence:

- `.research/assetto-live/ac-driving-final/evidence.json`
- `.research/assetto-live/ac-driving-pause/evidence.json`
- `.research/assetto-live/ac-driving-resume/evidence.json`
- `.research/assetto-live/ac-driving-exit/evidence.json`

Independent read-only native-page capture
`.research/live-assetto/AC-raw-20261021-100614.json` recorded changing RPM and
speed in second gear. Replaying those actual bytes through the production parser
and session produced an initial baseline followed by **19/19 fresh frames**;
results are in
`.research/live-assetto/AC-production-decoder-20261021-100614.json`.

AC controls were backed up before the temporary keyboard/mouse driving setup.
The original `controls.ini` was restored and its SHA-256 matched the backup
byte for byte after testing. No physical output device was qualified by this run.

## Actual ACC driving

The installed ACC **1.10.4** was driven in Practice at Nürburgring with the
Lamborghini Huracan GT3 EVO (`lamborghini_huracan_gt3_evo`). Its real
`AC2-Win64-Shipping.exe` published live status `2`, advancing original physics
packets, shared-memory version **1.9**, and a maximum of **8650 RPM**. The legacy
`acVersion` shared-memory field contains **1.7** on this build; that field is
recorded verbatim and does not identify the current game's displayed version.

The running development Cockpit verified the actual executable and delivered
all five read-only channels through an applied profile. A dedicated live capture
passed with **139/139 known samples**, changing RPM and speed, first gear, and
the published 8650 RPM maximum. A second moving capture passed with **90/90 known
samples**, RPM **1735–8192**, gears **0–3**, speed **32.313–141.928 km/h**, and
rev ratio **0.200578–0.947052**. Evidence:
`.research/assetto-live/acc-driving-final/evidence.json` and
`.research/assetto-live/acc-driving-moving/evidence.json`.

Independent read-only native-page capture
`.research/live-assetto/ACC-raw-20261021-101442.json` recorded **100 live samples**
with RPM **1730–8084**, gears **0–2**, and speed **30.645–134.383 km/h**. Replaying
those actual bytes through the production parser and session produced an initial
baseline followed by **99/99 fresh frames** with all five channels. Evidence:
`.research/live-assetto/ACC-production-decoder-20261021-101442.json`.

An earlier mixed driving/pause capture contained 202 known observations and 60
withdrawn observations. Its generic live-only 80% availability threshold failed
because the user paused during that recording; the dedicated live capture above
passed. Actual paused native pages continued advancing physics packet IDs while
reporting status `3`. The production decoder correctly published no fresh values
for all 20 paused raw samples, confirming that live-status gating remains
necessary even when physics counters advance. Evidence:
`.research/assetto-live/acc-driving/evidence.json`,
`.research/live-assetto/ACC-raw-20261021-101349.json`, and
`.research/live-assetto/ACC-production-decoder-20261021-101349.json`.

Dedicated pause and resume checks through the running Cockpit also passed.
Pause withdrew all five channels in **6/6 observations**. Resume restored all
five channels in **5/5 fresh observations**, reporting first gear, 1730 RPM,
8650 maximum RPM and a rev ratio of 0.2. Evidence:
`.research/assetto-live/acc-pause/evidence.json` and
`.research/assetto-live/acc-resume/evidence.json`.

After the user quit ACC, the actual Cockpit reported no matching executable
process, `processVerified: false`, no context session, and no current vehicle.
Before cleanup stopped the applied profile, it was degraded and waiting for a
fresh verified game context. Evidence:
`.research/assetto-live/acc-cleanup/evidence.json`. The earlier exit-series runner
expired at its 30-second deadline before the user quit, so
`.research/assetto-live/acc-exit/evidence.json` records a harness timing failure;
it does not establish a passing exit channel series or a native integration
defect. The subsequent cleanup removed the temporary profile.

No ACC controls were changed for this driving test.

## Actual EVO driving

The installed EVO **0.9.1** was driven in Practice with the **Abarth 695 Biposto**
on the selected Nürburgring Nordschleife layout. Native metadata reported
shared-memory version **1.0**, game version **0.9.1**, car name
`Abarth 695 Biposto`, and track name `Nurburgring`. The actual
`AssettoCorsaEVO.exe` published the independent `Local\acevo_pmf_*` mappings.
Live capture confirmed the char metadata, car at graphics offset 3086, track at
static offset 136, and integer current maximum RPM at physics offset 588.

The running development Cockpit verified this executable, discovered its actual
car, and delivered all five read-only channels through an applied profile. The
moving capture passed with **91/91 known samples**, RPM **2081–6118**, gears
**0–5**, speed **15.750–159.616 km/h**, maximum RPM **6200**, and rev ratio
**0.335645–0.986774**. Evidence:
`.research/assetto-live/evo-driving-final/evidence.json`.

Independent read-only capture
`.research/live-assetto/EVO-raw-20261021-102456.json` recorded **50 live samples**
with RPM **1310–4199**, gears **0–5**, speed **82.126–161.233 km/h**, and the
same 6200 RPM maximum. Production parser/session replay produced an initial
baseline followed by **49/49 fresh frames** with all five channels. Evidence:
`.research/live-assetto/EVO-production-decoder-20261021-102456.json`.

The first attempted driving recording preceded completion of the user's control
setup and included pauses; it recorded neutral idle values and failed the
live-availability and movement thresholds. During that transition, the producer
briefly zeroed physics engine fields while its HUD page retained engine values.
A subsequent full 800-byte physics capture during actual movement showed the
engine running, correct RPM/gear/speed, a 6200 RPM ceiling, and the focused car
matching the player. The successful moving profile capture above confirms the
existing offsets; no production fallback or layout change was introduced for
the control-screen transition. Evidence:
`.research/assetto-live/evo-driving/evidence.json`,
`.research/live-assetto/EVO-raw-20261021-102307.json`, and
`.research/live-assetto/EVO-diagnostic-20261021-102441.json`.

Dedicated pause, resume and exit checks through the running Cockpit passed.
Pause withdrew all five channels in **5/5 observations**. Resume restored all
five channels in **4/4 fresh observations**, reporting second gear, 1250 RPM,
6200 maximum RPM and rev ratio 0.201613. After the user quit the whole
application, **4/4 observations** had withdrawn channels, and both application
detection and verified driving-runtime presence were false. Cleanup then removed
the temporary profile. Evidence:

- `.research/assetto-live/evo-pause/evidence.json`
- `.research/assetto-live/evo-resume/evidence.json`
- `.research/assetto-live/evo-exit/evidence.json`
- `.research/assetto-live/evo-cleanup/evidence.json`

The user configured EVO control bindings during this run. The byte-for-byte
controls restoration documented above applies to the temporary AC configuration.

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
- All **14 game discovery regressions** passed, including the distinction
  between AC menu presence and an actual driving-runtime identity.
- All **8 live acceptance helper tests** passed, covering the live, pause,
  resume and exit assessment rules used for these captures.

Authored publisher samples establish native acquisition and profile delivery,
not compatibility with a particular real driving build. The separate actual AC,
ACC and EVO driving runs qualify the tested builds and sessions described above.
Physical output devices remain unqualified.
