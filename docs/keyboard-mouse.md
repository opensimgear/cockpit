# Keyboard and mouse mappings

Keyboard and mouse actions live in the same Profiles graph as controllers,
OpenSimGear panels and companion controls. Configure this computer's output
enablement in **Settings → Controller and desktop output**, then choose mappings
and scope in each profile's keyboard/mouse block.

## Author actions

Open a profile, choose a **Keyboard and mouse preset**, then **Add keyboard and
mouse**. Presets create an endpoint and any required visible processing blocks.
Connect exact source ports to those blocks or the endpoint's action ports.
A desktop-only profile needs no virtual controller.

The inspector supports physical key holds, timed taps, modifier chords, mouse
holds/clicks, scrolling, relative pointer movement and explicit key repeat.
Every action keeps its own typed port. Use **Update desktop actions** to put
edited action parameters into the graph, then Save the profile.

Keys identify physical positions. The same position can produce different
characters under US and German layouts. Left/right modifiers remain distinct.
The physical key picker stops the profile before taking focus. Unsupported keys
are refused. Text paste, media keys and program execution are not actions.

## Choose this block's scope

Each keyboard/mouse endpoint saves its own **Output scope**:

- **Selected game** uses the profile's verified game context. A game launch binds
  its actual started process independently of telemetry setup. Starting without
  a game context reports an error.
- **Selected application** uses **Use last focused external application**. Focus
  the target, return to Cockpit and capture it. Cockpit stores an immutable local
  observation and saves only an opaque reference in this block. Capturing an
  application for one profile cannot retarget another profile. The reference
  survives Cockpit restart; capture again if the target application itself
  restarts. An application captured explicitly stays selected during game launch.
- **Desktop-wide navigation** permits the profile's keyboard/mouse output across
  applications while it runs.

Scope changes and application captures update the draft immediately. **Save
profile** persists them. Native paths, process IDs and process creation times
stay local and are never accepted from a browser request. Imported references
unavailable on this computer require a fresh application capture.

Older profiles without a saved scope retain an application-only default: use
their explicit game context or the previously captured local application. A
legacy machine-wide desktop setting does not broaden an old profile's scope.

## Save, start and stop

Saving an inactive profile stores it; choose **Start profile** when ready.
Saving the currently running profile applies its changes automatically while
retaining game context and selection mode. If that update fails, the changes
remain saved and Cockpit offers **Retry saved changes**. Opening an editor,
importing and application capture do not start output.

Application-scoped output only runs while its exact target process is in front.
Focus loss cancels pending actions and releases this profile's holds. Returning
requires fresh released/neutral controls before rearming. **Waiting** means some
controls still need release or centering; already armed actions can continue.

**Stop** cancels pending work and prevents automatic rearming. Use **Start
profile** to run again, or explicitly select Automatic to resume automatic
selection. A mixed profile must pass the readiness checks of
every required output backend. Failed activation does not turn into recording.

## Debug and recovery

Open **Debug** on the Profiles list or editor for **Profile debug**. Run native
preflight there after graph changes. **Recording preview** uses sample controls
to show ordered operations, timing and shared-hold collisions without an OS
injector. Native desktop details, applied samples, path activity and retired
output recovery are in the same Debug view.

Two actions holding Control retain independent ownership; releasing one leaves
the other held. A timed tap colliding with an existing hold reports the collision
instead of manufacturing a release. Encoder bursts retain ordered taps with
down/up dwell. A stationary deflected stick keeps integrating pointer motion
while its source is alive.

Uncertain submission or interrupted workers retain a recovery record. Recovery
attempts releases for recorded possible holds and leaves output stopped; it does
not retry clicks or shortcuts. Physical keyboard holds share the OS stream with
injected holds. Windows pointer settings still affect relative motion, and a
game's native controller bindings can react alongside mapped keys.

Windows provides native keyboard/mouse submission. Linux/macOS support portable
authoring and recording. The [output contract](desktop-output-contract.md)
contains implementation details and recorded native verification evidence.
