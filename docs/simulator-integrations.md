# Simulator profiles and telemetry outputs

Milestone 4 uses one provider-neutral binding document and separate local connection settings/device assignments. Open **Integrations** to import or create a profile. DCS-BIOS supports named inputs and outputs from an installed aircraft catalogue. The Assetto Corsa Rally starter advertises telemetry outputs only. Other games can add acquisition/decoding providers without adding game-specific LED commands to firmware.

**Games** can capture a saved integration configuration in a game profile and
start it as part of coordinated activation or launch. The captured configuration
is independent of later edits in Integrations. On exit or deactivation, the game
coordinator restores the previous peripheral and integration setup; interrupted
sessions use explicit recovery. See [Games and hardware profiles](games.md).

Games provides DCS-BIOS installation and readiness checks on Windows; see
[Integration setup and readiness](games.md#integration-setup-and-readiness). Setup
prepares local connection defaults without changing portable bindings or starting
a runtime. Existing drafts opt in to copying the prepared settings.

## Portable and local settings

A portable JSON document contains `schemaVersion:1`, `id`, `name`, `provider`, `context`, logical `roles`, and `inputs`/`outputs`. Bindings reference a role, named simulator control/channel and an advertised physical input or endpoint/action. DCS output ordinals retain their original catalogue indexes. Local `connection` settings and explicit role assignments to `(gatewayId,moduleId)` stay outside the exported document. Identical module identities on separate gateways remain distinct targets.

Import is a bounded preview. It does not save, start, assign devices, acquire outputs or change controller publication. Assign each role explicitly and refresh device capabilities. **Validate** checks provider metadata, types, interfaces, ranges, source ordinals, exact endpoint/action identities and current routes. Review diagnostics, then **Save**. Saved configuration never starts automatically. **Start** rechecks the current catalogue, routes and devices; changed semantics or routes require explicit revalidation. Binary DCS addresses are resolved from installed files rather than copied into portable profiles.

Limits are eight logical roles, eight device targets on four gateways, 16 inputs and 16 outputs, and 64 KiB profile/request size. One integration runs at a time, including the existing DCS-BIOS editor. Stop before editing. Existing schema 1 master-caution and schema 2 DCS settings stay available and are not silently rewritten; their legacy local output guard does not provide the endpoint leases required by new portable profiles. Use Integrations for device-enforced ownership and temporary overrides.

## Output ownership and manual control

Portable bindings to device outputs require firmware implementing volatile endpoint leases. Start validates all targets and obtains every output claim before forwarding input commands. Claims are separate from configuration writers and HID ownership. Actions sharing one physical endpoint share its lock, including legacy LED diagnostics and one-shot aliases. A competing live owner cannot be displaced by a manual action or another integration.

Each run uses secret UUID capability tokens and device-issued lease generations. Tokens are not returned by ownership queries or published status. The device stores a descriptor-validated fallback and a lease lasting at most 5000 ms; Cockpit renews it every 1000 ms through that gateway's serialized worker. Device timers apply fallbacks if the host disappears. Reboot and transport route loss invalidate the relevant claims. Claims never save flash.

An owned output offers a temporary override of up to 30000 ms. The run retains its claim while telemetry continues updating behind the override. **Resume** or expiry applies the latest fresh value. Unknown/stale feedback cancels overrides and applies the configured fallback. Stop discards pending fresh values, releases only momentary presses actually sent by the run, applies output fallbacks and releases claims. Lost replies remain uncertain; readable state is reconciled before another setter. An unreachable endpoint cannot confirm fallback until readback or device-side expiry completes.

Every gateway has an independent I/O worker. Pending absolute setters coalesce to the newest desired value; ordered button/encoder events retain their order. A slow gateway does not stop telemetry acquisition or another gateway's inputs. Input forwarding requires fresh simulator state, a current event baseline and disabled direct/gateway HID for its input panels. A device used only for feedback may retain unrelated HID input.

## Output-only devices and rev lights

A device may advertise a valid revision with zero buttons, axes and hats. It joins normal inventory and supports configuration/actions without publishing an empty HID controller. Direct device USB exposes CDC only. Cockpit labels it as an output device and offers no Enable/Disable controller controls. Simulations provide RGB/boolean outputs and the current board's status LED using the same production contracts; see the device repository's output-only presets.

Formats include boolean/on-off enum LEDs, text/numeric displays with bounded encoding/cell/byte fit, numeric thresholds and up to eight ordered RGB bands. A Rally rev-light preset uses the derived `rev_ratio`, changing green/yellow/red at configured thresholds; a boolean or status LED uses a configured threshold. Bind several separate advertised endpoints for several lights. These are Cockpit rules over numeric channels, not an assumed native game rev-light command. Missing maximum RPM leaves `rev_ratio` unknown and drives fallback; an explicit `rpm` binding can use a chosen absolute threshold.

## Assetto Corsa Rally starter

Games can prepare the native connection and verify its exact executable against
a live telemetry session. Saving a native profile may defer the process ID until
Games launches it; direct manual Start still needs a running process. Setup never
installs a plugin or changes game files.

The native source is **Windows shared memory**, selected with the Rally process ID. It opens existing `Local\acpmf_physics`, `Local\acpmf_graphics` and `Local\acpmf_static` mappings read-only. The researched executable basename is `acr.exe`; the implementation verifies the selected process identity/creation epoch and checks its lifetime. Shared mapping names are reused by other Assetto games and alone do not establish the producer. Run only the selected game when using these mappings. The source closes old mapping handles on exit/reset and uses bounded stable-copy retries.

The adapter decodes explicit little-endian prefixes: physics packet ID at 0, raw gear at 16, RPM at 20 and speed km/h at 28; graphics status at 4; static maximum RPM at 412 and bounded UTF-16 context. Normalized channels are `rpm`, `gear` (reverse -1, neutral 0, forward 1+), `speed_kmh`, optional `max_rpm`, and derived `rev_ratio`. Live status and an advancing original physics packet ID establish freshness. Initial attachment, process/car/track changes and counter resets require a new baseline. Duplicate/frozen snapshots do not refresh the source; off, replay, pause, invalid or stale data falls back. Counter wrap is handled as unsigned progression.

This layout is developer-derived, not an officially guaranteed ACR binary interface. Sources: [ACR-specific MIT reader](https://github.com/LuizZak/AssettoCorsaRallyTelemetryReader/tree/8bb26990def38ffd0e4afe71e04e85e5ddeb3670), [independent graphics/status parser](https://github.com/decnet100/acr_telemetry/tree/eaef0f29c6712a2e0dbc3bfd8246b84dcdbb9586), and [Microsoft mapping lifetime documentation](https://learn.microsoft.com/en-us/windows/win32/memory/sharing-files-and-memory). No code was copied from the noncommercial recorder. Later conflicting fields are deliberately excluded. Static maximum RPM must be valid; an observed peak is not treated as a redline.

On every platform, **Synthetic** source accepts bounded authored physics/graphics/static byte pages through the identical parser and runtime. The editor supplies RPM, gear, speed, maximum RPM, progression and live/pause/replay/off controls. Synthetic snapshots are OpenSimGear test input; they are not Rally native UDP. Original Assetto Corsa has a separate [Kunos UDP interface](https://docs.google.com/document/d/1KfkZiIluXZ6mMhLWfDX1qAGbvhGRC3ZUzjVIt5FQpp4/pub) and remains a future independent provider.

Software checks and authored samples do not qualify live Rally versions, real DCS, Windows USB/HID enumeration or assembled LEDs/displays/CAN. Hardware validation is deferred for this milestone; board design versions remain 0.1.0.
