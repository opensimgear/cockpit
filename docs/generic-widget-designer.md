# Generic widget designer

Cockpit stores editable widget source and compiles finished visuals for browser
companions. The shipped library starts with **Toggle**. Existing fixed widgets,
saved panels and their endpoint mappings are deliberately reset; pairing, trusted
certificates, listener settings and unrelated profile data remain separate.
ESP32 pairing remains paused and compiled widgets cannot be assigned to ESP32.
That implementation belongs to feature #161 in Backlog.

The [state and signal-flow editor](widget-state-signal-editor-plan.md) extends
this layer designer. Toggle is the only shipped pilot; numeric gauges and three-state
components are qualification fixtures.

## Authoring and publishing

Open **Companions → Widgets**. Edit Toggle or choose **New** for a blank design.
The editor opens in the main content area; Back returns to the library or the
panel draft that opened it. States and Signals share the profile editor toolbar:
ELK Auto-layout, Fit all, zoom, background panning, and the global wheel/arrow
preferences in Settings. Layout stays outside compiled source identity.

The workspaces are:

- **Design**: shapes, paths, text, imported images, groups, transforms and typed
  parameters. Source preview shows editable geometry.
- **States**: state cards in independent regions, initial states and owned values,
  event cards with allowed updates, timers, and touch areas with press/release/
  activate/cancel connectors. Select a transition wire to edit Any state or Stay,
  priorities, guards and set/emit/cue/start-timer/cancel-timer actions. Motion and
  sound remain available beside the selected transition.
- **Signals**: typed local/input/feedback declarations, feedback freshness and
  quality, named profile ports, output neutrals, rendering samples, transforms
  and presentation bindings. Select a card for its complete contextual inspector.
  State owner references link back to States. Event connectors are diamonds and
  dashed wires; value connectors are circles. Data and Behavior have no separate tabs.
- **Motion**: optional baked property/transform tracks between named appearances.
  Numeric smoothing walks existing sampled frames.
- **Sound**: optional bounded PCM16 WAV assets and cue policies.
- **Try**: the current compiled artifact with independent input and feedback
  stimuli, quality, requested outputs and behavior state inspection.

Compile before Try, export or save. Any source, parameter, size or media-preference
edit invalidates the result immediately. Cancellation fences discard late results.
Undo restores source and still requires compilation. Graph layout is stored separately
and does not invalidate assets. Saving replaces the current widget by its stable ID.
There is no source revision history, format discriminator or backward-compatible
reader. The native library is simply `{ "definitions": [source] }`; panel instances
refer to `sourceId`. Imports use the same schema. A source edit invalidates saved
compiled panel assets that depend on it, so those panels must be compiled again.

In **Panels**, create a panel and add a source from the palette. Drag, resize or
enter exact pixel placement, set per-instance parameters and the current page's
Background color, then compile the panel. Page colors persist through export,
import, compiled Try and browser delivery. Widget surfaces and previews are
transparent; only authored artwork contributes pixels. Blank widgets start with
an unfilled shape. The panel color shows through empty widget space.

Select a connected destination under **Assignments** to check its current physical
display size, including browser display scaling. If the page is too small, Cockpit
shows a warning beside Width and Height before compilation. The same warning appears
in the companion's Cockpit details and on the destination. It is advisory: it does
not resize artwork or change compilation. Disconnected or unmeasured destinations
cannot supply a current size check. Display reports and active-panel warnings are
also available through MCP `companion_inventory` in `host.clients[].displayReports`.
The source preview is labeled. **Try** uses complete compiled widget surfaces;
its input stays in the local simulator. Select browser companions and **Save**
to publish and update their assignments. Export includes editable panel/source
data and its validated compiled result; import requires a fresh target compile.

The Toggle's thumb follows `requested` immediately. `confirmed` and `quality`
are separate signals; the text below shows simulator confirmation. Host submission
or local input does not count as game confirmation.

## Contracts and execution

`packages/widget-compiler` owns the plain authoring schema and the off-device compiler.
The optional `design` object contains `nodes`, `owners`, `appearances` and `effects`.
`lowerWidgetDesign` expands acyclic typed expression references, generates owned-value
writes and profile emissions, resolves appearance overrides and derives numeric motion
endpoints before the existing compiler runs. Source hashes cover the entire authored
document. Native admission checks the same contracts independently.

In Design, choose Base or a state appearance. Base is inherited by every state;
a state edit patches properties by stable layer ID. Groups, ordering and layer
structure remain global. Deletion and duplication update all appearance and motion
references together. Reset override restores inheritance; Apply to base promotes the
selected layer's patches. More-specific combined appearances override simpler ones;
independent regions writing the same property require an explicit combination.
Advanced expressions remain editable through the existing forms.

An owner maps each state to a local value and optionally a profile command. The
compiler emits the target literal, preserving the reducer's pre-event snapshot
semantics. Other actions cannot write that same owned signal or command. Pure graph
cycles, unknown references, type mismatches and out-of-range values are rejected.
`packages/widget-runtime` owns the bounded behavior, raster artifact and panel
contracts and player. Source is data: executable scripts, HTML, remote asset URLs
and arbitrary runtime code are rejected.

Panel schema 4 contains semantic endpoints, complete widget placements and
hash-addressed `osg-raster-widget-1` artifacts. Each artifact contains finished
16×16 RGB565 little-endian tiles with straight alpha, frame maps, static selectors,
hit regions, behavior and optional frame sequences/audio. It contains no authoring
layers, SVG, fonts or drawing commands. A tile is part of a finished appearance.
The browser copies pixels; it does not draw the authoring scene.

The compiler uses pinned resvg WASM and checked font bytes. Source, parameters,
behavior, capability profile, compiler and rasterizer identities enter the cache
contract. Assets and tiles carry SHA-256 hashes. Host and browser independently
check schemas, exact asset closure, bytes, hashes, selectors, port domains,
neutral values and resource arithmetic before activation. Saved draft admission
also compares source and parameter hashes to prevent reusing an earlier result.

The behavior reducer uses injected monotonic time. Each region selects at most
one transition per event, with explicit priority. Guards and actions use the same
pre-event snapshot; conflicting writes fail atomically. Feedback freshness includes
source age and arrival age. Reset cancels timers, suppresses cues and emits declared
neutral values. Presentation motion never delays or changes emitted controls.

Numeric presentation signals declare finite samples, maximum pixel travel and
accepted half-sample error. Raw validated values drive guards and thresholds.
The new linear RPM, radial RPM and three-position fixtures demonstrate the same
engine without widget-kind branches. They are test fixtures, outside the seed
catalog. More independent appearance dimensions increase the finite state product;
the compiler refuses an unbounded or over-budget family.

## Target limits and media fallbacks

Browser capability version 1 advertises the raster codec, dimensions, frames,
payload bytes, resident budget, motion and supported audio formats. Initial hard
limits are 2048 pixels per widget dimension, 4096 frames, 2 MiB per asset, 8 MiB
aggregate binary assets, a 1 MiB panel manifest and the existing 13 MiB transfer
envelope. Active plus candidate memory is checked against 64 MiB. Resource reports
include encoded payload and manifest allowances, frame maps, two pixel surfaces,
decode scratch and bounded concurrent audio decoding/playback; see the compiler
README for exact arithmetic. Browser allocator overhead and actual rendering
performance are qualified separately from these normative accounting values.

Motion/audio preferences are compile inputs. Unsupported or disabled media are
omitted and reported as fallbacks; static visuals remain mandatory. Disabling
motion snaps to the current appearance while gauges still consume live values.
The browser also honors reduced motion. Audio requires a real user gesture and
reports availability separately from capability. Muted, hidden, disconnected or
locked cues are dropped. Cooldown, stop-on-exit and a four-voice bound prevent
unbounded playback; unlocking or reconnecting never replays old cues.

The authenticated existing publication transfer carries immutable manifests and
assets. Warm browsers advertise verified public asset hashes and receive missing
payloads. The local cache grants no authority and is independently reverified.
Steady operation sends semantic input/feedback and existing session heartbeats,
without graphic updates. Blur, hiding, timing gaps, errors or lost authority stop
the session and require a neutral baseline before resuming.

## Verification

- `pnpm check`: TypeScript, Svelte and generated native source parity corpus.
- `pnpm widgets:coverage`: behavior/player/compiler/artifact/panel/source-store
  tests, real WASM goldens and malformed imports, with 71% coverage gates.
- `pnpm browser:test`: authentication, cache, lifecycle, feedback and compiled
  pixel/semantic regression tests.
- `pnpm companion:test`: shared contracts and existing input/session regressions,
  including the new widget coverage gate.
- `python scripts/companion-coverage.py`: native independent admission, service,
  source library and cutover coverage.
- `node --test --test-force-exit tests/ui/widget-state-signal-native.mjs`: capsule
  state appearances, graph wiring, layout identity, compiled traces, save/reopen and
  browser raster playback using the running native Cockpit.
- `pnpm widgets:native:ui`: the actual running dev Cockpit at CDP 9222 and the
  production browser companion. Records screenshots, exact finished-pixel checks,
  interaction, optional media, cache and rendering evidence under `.research/`.

Legacy source publication validation is isolated behind an explicit test-fixture
feature for transport regression fixtures. The public admission and publication
commands reject legacy schemas in every build, including tests. Normal production
builds contain the compiled publication path only.
