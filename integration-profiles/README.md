# Integration profiles

[`x-plane-c172-panel.json`](x-plane-c172-panel.json) demonstrates an X-Plane
indicator and numeric airspeed display. Assign local devices and replace example
endpoint/action IDs with advertised hardware capabilities before validation.
See the [X-Plane guide](../docs/x-plane.md) for configuration and qualification limits.

Open **Integrations** for Milestone 4 portable profiles. Exported JSON contains
the provider, logical device roles and named input/output bindings. Assign roles
to local gateway/device identities separately, refresh capabilities, then
Validate, Save and explicitly Start. Import previews never start a profile or
assign devices. The native service persists local settings without autostart.

DCS-BIOS uses an installed aircraft catalogue. Assetto Corsa Rally starts with
RPM, gear, speed, maximum RPM and derived rev-ratio telemetry, numeric thresholds
and RGB rev-light bands. The synthetic source uses the same byte parser and
device contracts as the Windows shared-memory starter. Portable profiles require
device-enforced endpoint leases; temporary overrides retain those claims and
Resume applies the latest fresh telemetry. Output-only devices need no HID
controller. See [the simulator integration guide](../docs/simulator-integrations.md)
for schema limits, ownership, fallback and recovery. This directory does not
contain bundled profile packages.

## Existing DCS settings

The **DCS-BIOS** editor retains older saved master-caution and typed profiles.
Its local output guard is a compatibility path; use Integrations for endpoint
leases and portable role assignments. It supports up to 16 typed input and
16 output bindings across eight known modules on four gateways. Aircraft
catalogues and advertised device capabilities determine supported bindings.

Open **DCS-BIOS** in the sidebar, enter the installed DCS-BIOS `doc/json`
directory and search its named controls. Choose a gateway-scoped device for each
binding. Refresh advertised capabilities before selecting momentary contacts,
maintained switches, encoder ordinals, calibrated axes, LEDs or bounded string
displays. Supported command interfaces and output types come from the installed
catalogue; binary feedback addresses are resolved by the native service.

The Pico debug gateway's master-caution and radio presets use the same editor,
with catalogue-validated test bindings and LED/display readback. Add the virtual
panel on its gateway card, refresh its advertised capabilities, then choose its
A-10C test preset. Use the installed DCS-BIOS catalogue for a live aircraft; the
authored development peer's addresses are synthetic.

Input devices require disabled gateway HID, inactive direct USB and finished
setup sessions. **Disable controller** uses the visible USB reconnect warning;
wait for the update to finish. Devices used only for feedback may retain unrelated
enabled USB inputs. Devices advertising no inputs have no HID controller.
Buttons and switches send explicit states, encoders send ordered
steps, and axes scale independently calibrated samples into the command range
with a threshold in target units. Selector positions are not offered as switches.

LED feedback uses a readable boolean setter or current-board status LED on/off
mode. Text, rounded integer and fixed decimal formatting use readable bounded
string setters. Device display metadata sets the cells, encoding and byte limit;
formatting supports prefixes, suffixes, alignment and whole-character truncation
or rejection. Unknown and stale text default to blank, and LED fallback is Off.
Desired and applied values are reported by the native formatter.

Save persists the profile; Start is explicit after saving and every application
restart. Stop before editing. An unchanged saved Milestone 2 master-caution
profile can still Start; its two bindings are upgraded only by explicit Save.
The **Add master-caution preset** button preserves that setup path. Global Stop
remains available when a panel is offline.

A Rust background service consumes ordered input events and complete DCS export
frames while Cockpit is open. New baselines discard historical commands. Held
momentary buttons must release before a new press. Maintained switches and
available axes synchronize their current absolute state at each fresh baseline. Disarming sends one neutral release
only for a press already forwarded by this profile, and uncertain command sends
are never replayed. Absolute display/LED writes coalesce to the newest value;
readback reconciles uncertain setters. Per-binding status and per-gateway traffic
metrics expose readiness and applied output state.

Use [the local UDP peer](../simulation/dcs-bios/README.md) and its
[synthetic catalogue](../simulation/dcs-bios/catalogue/) for software development.
The sample addresses are not a real aircraft catalogue. The simulated master
caution uses the same current-board button/event/status LED contract; display
simulations use the shared advertised endpoint contract. Assembled hardware and
live DCS qualification remain deferred.
