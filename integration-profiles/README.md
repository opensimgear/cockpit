# Integration profiles

Milestone 2 implements one user-saved A-10C master-caution profile in Cockpit's
native application settings. This directory does not contain portable profile
packages yet; wider aircraft profiles and distribution are tracked in
[Milestone 4](https://github.com/opensimgear/cockpit/issues/16).

Open a connected CAN device's **DCS-BIOS** tab, disable its HID controller through
the visible confirmation flow, wait for layout completion and disconnect active
direct USB input. Select a zero-based mapped momentary button and either a
readable boolean LED setter or `led.status.set_mode` (`on`/`off`). Supply the
installed DCS-BIOS `doc/json` directory. The native loader resolves
`UFC_MASTER_CAUTION`, `MASTER_CAUTION`, aircraft aliases and export metadata by
name rather than storing a fixed feedback address.

Save persists the single profile; Start is explicit after saving and after every
application restart. Stop before editing it. Global Stop remains available when
the bound panel is offline. A Rust background worker consumes ordered panel
events and fresh DCS-BIOS export while Cockpit is open. New baselines discard
history, and a held button must release before a fresh press. Stale feedback
requests LED off and uncertain command sends are not replayed.

Use [the local UDP peer](../simulation/dcs-bios/README.md) and its
[synthetic catalogue](../simulation/dcs-bios/catalogue/) for software development.
The sample addresses are not a real aircraft catalogue. The simulated
master-caution device uses the same current-board button/event/status LED
contract. [Verification](../docs/verification/2026-10-13-dcs-bios-master-caution.md)
records software evidence; assembled hardware and live DCS qualification remain
deferred.

Maintained switches use the same release-before-arm startup policy: their initial
position is not sent. Release a held switch before the next press can command DCS.
