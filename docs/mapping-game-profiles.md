# Panel events and retained game ownership

Panel events are explicit physical input ports in [unified Profiles](profiles-guide.md).
Local resource assignment chooses the exact USB/CAN route and native capability
token. Each subscription checks identity, route/lease, module boot and event
stream. Buttons preserve ordered edges; axes consume current calibrated state.
A gap, reboot or lost worker invalidates dependent contributions and requires a
fresh baseline. Merely inspecting a resource does not disable HID or change
permanent device settings.

The separate game-profile mapping capture, migration and recording-resume workflow
is retired. Games launches the selected saved unified graph. Existing schema-1/2
game configuration, mapping snapshots and integration settings are not imported,
rewritten or used as fallback configuration.

## Cleanup of an interrupted old session

An existing game recovery journal can still describe hardware, visibility,
virtual-output or companion ownership that needs releasing. Games exposes this
as **retained cleanup**, independently of current profile authoring. Native
cleanup uses its exact owner and journal fingerprint under an exclusive file
lock. Changed, malformed, unsafe or ambiguous records remain visible and block
new ownership until resolved explicitly.

Cleanup neutralizes owned output and restores only matching captured state. It
never starts a retired provider or mapping, resumes a prior recording, restores
script timers, captures a new baseline or reapplies hiding. Explicitly keeping
the current setup archives eligible evidence without replaying configuration.
Uncertain writes and external changes require inspection; cleanup does not retry
them as new starts. Existing output and visibility qualification gates apply.

The [Games guide](games.md) describes current launch and recovery, and the
[unified contracts](unified-profiles.md) describe fresh storage. The
[M6 verification record](verification/2026-10-15-panel-game-m6.md) remains a
historical account of the former workflow, not the current authoring interface.
