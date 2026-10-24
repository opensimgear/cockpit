# Keyboard and mouse mappings

Keyboard and mouse actions live in the same Profiles graph as controllers,
OpenSimGear panels and companion controls. Configure this computer's output
enablement in **Settings → Controller and desktop output**, then choose mappings
and scope in each profile's keyboard/mouse block.

## Author actions

Open a profile and choose **Keyboard and mouse** in the **Components** palette.
Select the added block to configure its starter action, then connect exact source
ports to its action ports. Add transformations through connection lines when needed.
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

- **Context scoped** uses the profile's saved game context. Bindings work only
  while that game's verified process is in front. When it closes, bindings stay
  inactive until the game returns. Cockpit binds the new verified process and
  waits for released/neutral controls before rearming. Telemetry setup is not
  required for process scoping.
- **Global** permits keyboard/mouse output across applications while the profile
  is active. Game exit does not disable global bindings.

Scope changes update the draft immediately; **Save profile** persists them.
Existing application scopes remain readable for older profiles; choose either
current scope when editing them. Native process identity stays local.

## Save, activate and deactivate

Saving an inactive profile stores it and shows a notification; choose **Activate**
on its tile when ready. Saving the active profile applies its changes using the
saved game context and retaining selection mode. If that update fails, the
changes remain saved; deactivate and activate the saved profile to retry.
Opening an editor and importing do not activate output. If another profile is
active, activation asks for confirmation before switching profiles.

Application-scoped output only runs while its exact target process is in front.
Focus loss cancels pending actions and releases this profile's holds. Returning
requires fresh released/neutral controls before rearming. **Waiting** means some
controls still need release or centering; already armed actions can continue.

**Deactivate** cancels pending work and prevents automatic rearming. Cockpit
then shows **No profile active**. Use **Activate** again, or explicitly select Automatic to resume automatic
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
injected holds. Releasing a mapped key can send key-up while the same physical
key is still held; releasing the physical key can likewise clear a mapped hold.
The ownership ledger keeps Cockpit actions separate, but cannot isolate physical
keyboard state. Windows pointer settings still affect relative motion, and a
game's native controller bindings can react alongside mapped keys.

Windows provides native keyboard/mouse submission. Linux/macOS support portable
authoring and recording. The [output contract](desktop-output-contract.md)
contains implementation details and recorded native verification evidence.
