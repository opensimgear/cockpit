# Windows game discovery and scoped telemetry permission

General Settings controls the shared Windows process discovery watcher. With
that preference off, the approved one-second scan still discovers the game.
Assetto and X-Plane separately use a scoped Windows process-event watcher to
detect competing simulator instances before they attach a telemetry reader.
Assetto's shared-memory pages do not identify the producer. The process
discovery scan therefore cannot stand in for the scoped telemetry check.

The Games and telemetry views now explain this distinction beside **Allow**.
That action can open Windows consent for the exact running game. Starting a
game and opening the details never requests elevation on its own.

The native fixtures now select the main Cockpit WebView using the shared CDP
helper. The expanded Assetto fixture also uses the current installation
registration and selection commands. It removes only its private installation
after the run and restores the previous selection.

The permission-denied scenario runs with disposable Cockpit settings and
authored Windows publishers. It checks that the fallback discovers the game,
the inspector withholds current values, and the scoped **Allow** action is
visible in Games. It also requires Assetto setup to stay unverified and
X-Plane to emit zero RREF subscription requests under denial. Run it on a
Windows account that denies the scoped event subscription, with no simulator
instances running and the native dev Cockpit started with CDP enabled.

```powershell
$env:OSG_TELEMETRY_PERMISSION_DENIED = '1'
$env:OSG_ASSETTO_TELEMETRY_NATIVE = '1'
node --test tests/ui/assetto-telemetry-native.mjs
$env:OSG_XPLANE_TELEMETRY_NATIVE = '1'
node --test tests/ui/xplane-telemetry-native.mjs
```

The X-Plane fixture uses the complete installed SDK `DataRefs.txt` or the
path set in `OSG_XPLANE_DATAREFS`. Both fixtures use `OSG_NATIVE_CDP_URL`
when supplied. The separate granted-access and helper lifetime checks remain
in [#199](https://github.com/opensimgear/cockpit/issues/199).

Both X-Plane versions and Assetto Corsa, Competizione, EVO and Rally passed
in the running Windows dev Cockpit under WMI error `0x80041003`. The
X-Plane and Assetto records are under
`.research/xplane-telemetry-native/1794164879944/` and
`.research/assetto-telemetry-native/1794165027494/`. Both restored their
private installations without a cleanup error. The 1,714 portal tests and
the Games coverage suite passed. Svelte reported zero diagnostics and the
frontend production build passed.
