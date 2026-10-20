# Simulator ports in unified profiles

Configure installation, provider connection, catalogue/aircraft and freshness in the owning game's [details](games.md#integration-setup-and-readiness). DCS-BIOS settings belong in DCS details. **Profiles** owns selected resource ports, exact directed bindings, visible processing and output policies. The independent integration/DCS configuration editors, global feedback saves and standalone Start paths are retired; old configurations are not read or migrated.

DCS-BIOS advertises actual catalogue-backed command inputs and telemetry outputs. Assetto Corsa Rally and X-Plane advertise supported read-only telemetry. Read-only providers cannot acquire invented command ports. The [X-Plane guide](x-plane.md) records its setup, channels and qualification limits.

## Author and apply

1. Open **Profiles**, then Add or open a graph. Select a game resource and choose exact catalogue output/command ports. Closed-game catalogues support authoring without starting a provider.
2. Assign exact physical/companion resources and select their actual input/output contracts. USB/CAN and gateway attachment stay resource attributes, not graph connections.
3. Connect exact ports. Add visible unit/range conversions, comparisons/hysteresis, formatting and timing nodes explicitly. Actions require complete ordered arguments through an explicit invocation/argument-pack node; a Boolean or event cannot silently become an action.
4. Inspect draft preflight, resolve diagnostics and **Save profile**. Saving does not start a provider or send output. Portable schema-2 graphs are bounded to 512 KiB; local assignments and provider connection settings are separate. See the [profile contract](unified-profiles.md) for collection bounds.
5. **Apply saved revision** with a matching verified game context, or use Games **Launch** with the saved Manual pin or Automatic rule/default. Native admission checks the actual process path/PID/creation epoch, setup/catalogue revisions, current contracts and output ownership. A profile may run ordinary hardware with no game while its integration branches remain dormant.

Provider edits revoke incompatible captured execution and require fresh native revalidation. Missing/changed ports are reported rather than rebound by name. Wrong-game packets cannot arm a dormant branch. Game exit, process replacement and stale telemetry invalidate affected values and dependent contributions while unrelated admitted hardware branches can continue. Applied inspection retains freshness and requested-output provenance; it does not prove a physical change or simulator acknowledgement.

## Output ownership and manual control

Physical output needs actual advertised volatile endpoint ownership. The profile adapter requires a readable Setter with compare-if-unchanged restoration and atomic capture-at-claim fallback. It pins route, boot, configuration, catalogue and action contract, then captures the actual prior state when claiming the endpoint. Its bounded five-second lease and private renewal worker do not write flash. Unsupported endpoints remain unavailable; a manual action cannot displace an active owner.

One-shot aliases may use a compatible owned Setter on the same endpoint. Ordered named arguments retain their exact types. Ambiguous claims, invocations and renewals revoke delivery without retrying the operation. Source loss, Stop and switching release affected owned contributions. Cleanup restores only still-owned state and preserves external edits; an unreachable endpoint cannot confirm restoration until readback or device-side expiry. See [physical leases](profile-physical-actions.md) and the [bounded worker](profile-physical-action-worker.md).

Inputs and output actors use existing per-endpoint USB/CAN workers, so one stalled route does not drive the reducer from the UI or stall unrelated endpoints. Shared writer/identity checks remain required. Direct DCS/HID ownership constraints are enforced independently of OS hiding. Firmware publication, physical connectivity and routing activity are separate facts; persistent Enable/Disable is not a temporary profile override.

Interrupted ownership uses explicit profile recovery or retained previous-system cleanup. Recovery cannot replay commands, macros, held input, an old integration graph or hiding. Optional dependencies and current qualification gates remain unchanged.

## Output-only devices and rev lights

A device may advertise a valid revision with zero buttons, axes and hats. It joins normal inventory and supports configuration/actions without publishing an empty HID controller. Direct device USB exposes CDC only. Cockpit labels it as an output device and offers no Enable/Disable controller controls. Simulations provide RGB/boolean outputs and the current board's status LED using the same production contracts; see the device repository's output-only presets.

Declared destination contracts can include Boolean/enumerated LEDs and bounded text/numeric/RGB actions. Select only the actual supported endpoint and encode every conversion or condition in visible graph nodes. A rev-light condition can use `rev_ratio` or an explicit absolute RPM threshold; no native game rev-light command or hidden formatting preset is inferred. Missing maximum RPM leaves `rev_ratio` unknown and requires the authored unavailable/fallback behavior. Several lights require several explicitly selected endpoints/routes.

## Assetto Corsa Rally starter

Game details prepare the native connection; the profile coordinator verifies its
exact executable against a live telemetry session. Process identity is obtained
natively at launch/attachment, never saved as a durable profile PID. Setup never
installs a plugin or changes game files.

The native source is **Windows shared memory**, selected with the Rally process ID. It opens existing `Local\acpmf_physics`, `Local\acpmf_graphics` and `Local\acpmf_static` mappings read-only. The researched executable basename is `acr.exe`; the implementation verifies the selected process identity/creation epoch and checks its lifetime. Shared mapping names are reused by other Assetto games and alone do not establish the producer. Run only the selected game when using these mappings. The source closes old mapping handles on exit/reset and uses bounded stable-copy retries.

The adapter decodes explicit little-endian prefixes: physics packet ID at 0, raw gear at 16, RPM at 20 and speed km/h at 28; graphics status at 4; legacy static maximum RPM at 412 and bounded UTF-16 context. The optional 592-byte physics prefix adds current maximum RPM as int32 at 588. A valid current maximum takes precedence, with valid static maximum as fallback. Normalized channels are `rpm`, `gear` (reverse -1, neutral 0, forward 1+), `speed_kmh`, optional `max_rpm`, and derived `rev_ratio`. Live status and an advancing original physics packet ID establish freshness. Initial attachment, process/car/track changes and counter resets require a new baseline. Duplicate/frozen snapshots do not refresh the source; off, replay, pause, invalid or stale data falls back. Counter wrap is handled as unsigned progression.

This layout is developer-derived, not an officially guaranteed ACR binary interface. Sources: [ACR-specific MIT reader](https://github.com/LuizZak/AssettoCorsaRallyTelemetryReader/tree/8bb26990def38ffd0e4afe71e04e85e5ddeb3670), [independent graphics/status parser](https://github.com/decnet100/acr_telemetry/tree/eaef0f29c6712a2e0dbc3bfd8246b84dcdbb9586), and [Microsoft mapping lifetime documentation](https://learn.microsoft.com/en-us/windows/win32/memory/sharing-files-and-memory). No code was copied from the noncommercial recorder. The current maximum extension was confirmed against the [captured live build](verification/2026-10-21-acr-current-max-rpm.md); other unqualified fields remain excluded. If neither published maximum is valid, `max_rpm` and `rev_ratio` remain unknown; an observed peak is not treated as a redline.

Portable tests feed bounded authored physics/graphics/static byte pages through the same parser, including RPM, gear, speed, maximum RPM, progression and live/pause/replay/off states. These synthetic fixtures are OpenSimGear test input, not Rally native UDP or a second desktop integration editor. Original Assetto Corsa has a separate [Kunos UDP interface](https://docs.google.com/document/d/1KfkZiIluXZ6mMhLWfDX1qAGbvhGRC3ZUzjVIt5FQpp4/pub) and remains a future independent provider.

Software checks and authored samples do not qualify live Rally versions, real DCS, Windows USB/HID enumeration or assembled LEDs/displays/CAN. Hardware validation is deferred for this milestone; board design versions remain 0.1.0.

### Live ACR compatibility correction (2026-10-14)

A live Windows driving session exposed a sparse variant: graphics packet IDs
advance but status stays 0; both static version strings and maximum RPM are zero,
while car/track, RPM, gear and speed are populated. Current layout revision
`acr-prefix-3` retains the sparse-variant support introduced by `acr-prefix-2`:
it accepts this variant only with both version strings empty, nonempty car/track and valid required
channels, then requires the usual baseline and advancing physics packets. Other
status-0 frames remain inactive; explicit replay/pause values still disarm.
Native setup records the executable fingerprint instead of demanding missing
version strings for this variant. Diagnostics explicitly report that menu/pause/
replay discrimination is unavailable if the game continues publishing packets
without a state flag. Missing both current physics and legacy static maximum
still disables rev-ratio. This supersedes
the earlier universal assumption that live Rally always reports ACC status 2.

The actual Windows acquisition and complete native setup probe passed against
executable SHA-256 `42c359b0cd24774a86441710fdc7d7db3bc06b57b3ff30cb9ea5d56fb71448fb`.
Observed live values included 5156 RPM, first gear and 34.6244 km/h, Lancia Stratos
HF at Wales Afon Bidno. This verifies reception on that executable; pause/replay
transitions and dashboard calibration have not been qualified.

The [2026-10-21 maximum RPM correction](verification/2026-10-21-acr-current-max-rpm.md)
extends native acquisition to the current physics maximum and records layout
revision `acr-prefix-3`. On the same executable fingerprint, ten advancing
samples reported 8500 at physics offset 588 while static maximum remained zero.
