# Integration profiles

Cockpit saves one A-10C DCS-BIOS profile in its native application settings.
Milestone 3 supports up to 16 typed input and 16 output bindings across eight
known modules on four gateways. Portable profile packages, wider aircraft
coverage and ownership workflows remain [Milestone 4](https://github.com/opensimgear/cockpit/issues/16).
This directory does not contain importable profile packages.

Open **DCS-BIOS** in the sidebar, enter the installed DCS-BIOS `doc/json`
directory and search its named controls. Choose a gateway-scoped device for each
binding. Refresh advertised capabilities before selecting momentary contacts,
maintained switches, encoder ordinals, calibrated axes, LEDs or bounded string
displays. Supported command interfaces and output types come from the installed
catalogue; binary feedback addresses are resolved by the native service.

Input devices require disabled gateway HID, inactive direct USB and finished
setup sessions. **Disable controller** uses the visible USB reconnect warning;
wait for the update to finish. Output-only targets can retain their enabled USB
controllers. Buttons and switches send explicit states, encoders send ordered
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
