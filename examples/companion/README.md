# Importable companion examples

Import a JSON bundle through **Companions → Panels → Import panel**, then save and preview locally. See the [user guide](../../docs/companion-guide.md) for panel assignment, browser pairing and profile-owned control/feedback routes. The flight and racing files are bundleVersion 1 with a schema 2 designer document and no assets. All examples omit host addresses, credentials, installation identities, local slots and simulator connection settings.

| Bundle | Logical controller shape | Pages |
| --- | --- | --- |
| [Flight button box](flight-button-box.json) | 2 axes (unipolar flaps, bipolar trim), 7 buttons, 1 hat | Flight controls / Flight feedback |
| [Racing axes](axis-racing.json) | 4 axes (bipolar steering; unipolar throttle, brake and handbrake), 2 buttons | Driving controls / Driving feedback |
| [Shared systems complete](shared-systems-complete.bundle.json) | Reusable controls, confirmed feedback, instruments and decoration; schema 3 | All twelve catalog families |

The logical target shapes are complete. Unified profiles select exact declared companion ports and explicitly route them into chosen virtual layouts or supported actions; these examples never allocate or select a native device. Saving/importing a panel does not start output. Start with local preview; the [qualified release matrix is currently empty](../../docs/companion-release-matrix.md).

## Shared systems widgets

The complete panel contains an illuminated momentary button, toggle, selector,
status indicator, numeric display, linear and radial gauges, frame, divider,
page navigation, text instrument and power icon. Each is an original editable
layer composition using the pinned portable fonts. Browser and Android use the
same renderer as Cockpit preview; ESP32 uses LVGL with the same definitions,
properties and typed ports. Assignment checks the connected board's reported
font, geometry and resource limits and reports incompatibility explicitly. The
complete panel exceeds the compact board's component budget; use fewer instances
or pages appropriate to its reported limits.

The text instrument accepts typed text feedback. The power icon's editable
ellipse and stem illuminate only from fresh game-confirmed simulator feedback.
Pressed/requested/local values do not establish an active confirmed indicator.
No Star Citizen integration, GameGlass artwork or external font service is
included. Configure supported sources explicitly in Profiles.

The [earlier ten-widget panel](shared-systems.bundle.json) is retained byte for
byte so archived source identities remain reproducible. Regenerate both bundles
and their native publication fixtures using
`node scripts/widget-publication-fixtures.mjs`; add `--check` to verify them.
See [reusable widget authoring](../../docs/companion-guide.md#design-and-reuse-widgets)
for immutable revisions, per-instance properties and portable library bundles.

## Flight button box

**Transmit** and **Auxiliary** are momentary buttons. **Gear request** and **Lights request** toggle their requested virtual button state; they do not establish the aircraft's actual state. **Acknowledge** emits a 100 ms pulse. **Radio preset** emits one 50 ms button pulse in the selected direction. **View hat** uses eight directions and returns to neutral. **Flaps request** selects a unipolar value of 0, 0.5 or 1. **Trim** is a bipolar spring slider returning to 0.

The second page has a boolean gear-request indicator, radio-frequency text, a trim-requested numeric display (-1 to 1, `ratio`) and engine RPM gauge (0 to 100000, `rpm`). These stay unknown until routed. Suggested host visual routes:

| Published output/channel | Compatible source |
| --- | --- |
| `example-flight-gear-state-output` / `example-flight-gear-requested` | Requested or host-accepted control `example-flight-gear` (boolean state). |
| `example-flight-trim-state-output` / `example-flight-trim-requested` | Requested control `example-flight-trim` (number, `ratio`). |
| `example-flight-radio-text-output` / `example-flight-radio-text` | A supported DCS-BIOS string source selected from the installed aircraft catalogue. |
| `example-flight-rpm-output` / `example-flight-rpm` | A supported numeric RPM source whose full range fits 0 to 100000 `rpm`, or an explicitly validated conversion. |

Game bindings and telemetry vary by aircraft. Choose actual supported catalogue channels rather than inventing a generic radio or engine source. A request indicator is deliberately labelled Requested, not Game confirmed.

## Racing axes

Steering, throttle, brake and handbrake are independent spring sliders; several pointers can hold them simultaneously. Steering returns to 0 at centre; the three unipolar controls return to 0 on release. Shift up/down are momentary buttons. These are touch controls, not a claim of physical pedal or force-feedback behavior.

The RPM gauge is on the controls page. The second page offers speed (0 to 2000 `km/h`), normalized gear (-1 reverse, 0 neutral, up to 31; `ratio`) and steering-requested feedback (-1 to 1, `ratio`). Suggested sources are ACR catalogue `rpm`, `speed_kmh` and `gear`, plus requested control `example-racing-steering`. Configure ACR in its Games details, then capture the exact companion panel in Profiles and connect selected telemetry outputs to its declared feedback inputs. Add visible unit/range conversion where needed; save and Apply explicitly. Synthetic fixture values remain Local and cannot confirm a game value. This example adds no unsupported ACR text/car-name channel.

## Cancellation and editing

Stop, page/orientation changes and lifecycle cancellation reset all controls to their declared fallbacks: buttons false, hat -1, axes 0 in these two examples. Toggle/selector values do not survive Stop. Looking at the feedback page therefore stops input; explicitly Start again on returning to controls. Adapt behavior in the designer only after reviewing the game binding you intend to use.

Stable control and target IDs preserve bindings when restyling. A new publication hash is always explicit. Input behavior/fallback changes also change the capability fingerprint and require reconciliation. Two instances of the same UI get separate host instance identities; do not edit document IDs to impersonate another installation.

## Reproduce and validate

From the repository root with Node 24:

```text
node examples/companion/layouts.mjs
node --test examples/companion/layouts.test.mjs
```

The generator uses the production designer exporter. The tests compare exact checked-in bytes, import/save/reopen/export/publish both bundles, verify full target bindings, exercise simultaneous axes and button/pulse/hat cancellation, validate typed feedback aging without input, and reject machine/authority fields. They are deterministic software checks, not real multitouch, LAN, signed install, driver or game qualification.
