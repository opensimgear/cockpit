# Keyboard and mouse mappings

Keyboard and mouse actions live in the same Profiles graph as controller,
OpenSimGear panel and companion controls. Saving a draft, importing a profile,
opening the editor and restarting Cockpit do not send OS input.

## Author and record

Open a profile and choose a **Keyboard and mouse preset**. The presets add an
endpoint and any required visible processing blocks. Connect exact source ports
to those blocks or to the endpoint's action ports. A desktop-only profile needs
no virtual controller. The action inspector supports physical key holds, timed
taps, modifier chords, mouse holds/clicks, scrolling, relative pointer movement
and explicit key repeat. Every action keeps its own typed port.

Keys identify physical positions. A physical Y key produces different characters
under US and German layouts. Left/right modifiers remain distinct. Use the key
picker while it owns focus; Cockpit pauses the profile before capture. Unsupported
keys are refused. Text paste, media keys and program execution are not actions.

Refresh native preflight after changing the graph. **Recording preview** uses
sample controls to show ordered operations, timing and shared-hold collisions.
Recording has no native injector. A successful recording is separate from a
successful Windows submission and from observed behavior in a game.

## Development acceptance

Production Windows activation remains disabled while the qualification matrix is
incomplete. For explicit development acceptance, build the normal desktop app
with `OSG_DEV_DESKTOP_INPUT=1` in a Windows debug build. Its desktop status says
**Development acceptance mode**. This gate is independent of the existing
virtual-controller, hiding and live-script gates. A release build keeps desktop
activation closed even if that environment variable is present.

Enable desktop output in Profiles. The default scope is a selected application:
focus the target, return to Cockpit, then choose **Use last focused external
application**. Scope uses that exact process and its creation identity. A
verified game launch can bind its actual process independently of telemetry
setup. **Desktop-wide navigation** is a separate explicit scope choice. Paths,
processes and machine enablement stay local rather than entering exported
profiles.

Save and explicitly Apply the profile. Apply captures its revision; later saves
do not change the running graph. A mixed profile must pass every requested
backend's independent readiness checks. A failed live activation never silently
turns into recording preview.

Focus loss cancels pending actions and releases this profile's holds. Returning
requires a fresh released/neutral control before rearming. **Waiting** means
some controls still need to be released or centered; actions already armed can
continue. Stop is available in
Profiles and from the tray as **Stop keyboard and mouse output**. It pauses the
shared profile session and suppresses automatic rearming. Resume and Apply stay
explicit.

## Ownership and recovery

Two actions holding Control retain independent ownership. Releasing one cannot
release the other's contribution. A timed tap colliding with an existing hold
reports the collision instead of manufacturing a release. Encoder bursts retain
ordered taps with down/up dwell; an overfull queue faults visibly. A stationary
deflected stick keeps integrating motion while its native source remains alive.

An uncertain submission or interrupted worker leaves a bounded recovery record.
Cockpit does not retry an uncertain click or shortcut. Explicit recovery attempts
releases only for the recorded possible holds and leaves output stopped. Killing
the entire process tree or losing the Windows desktop can prevent cleanup.
Physical keyboard holds share the OS stream with injected holds; Cockpit cannot
isolate those physical holds. Windows pointer settings continue to affect relative
motion, and native controller game bindings can react alongside mapped keys.

Linux/macOS support portable authoring and recording. Their native submission
backends, advanced sequences/assignment rotation and live script desktop actions
remain separate work. The [contract and evidence](desktop-output-contract.md)
records the verified behavior and outstanding native/game qualification.
