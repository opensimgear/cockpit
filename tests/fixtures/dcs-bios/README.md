# DCS-BIOS test catalogue

These small, authored fixtures reproduce the category/control/inputs/outputs JSON
shape emitted by DCS-Skunkworks DCS-BIOS at commit
`11c6209beca4e5aae201a5cf62a7c346ebabbfc8`. They are software test data, not an
installed aircraft catalogue. The feedback address `0x1234` and mask `0x0040`
are deliberately illustrative: production resolves these values from the user's
installed `doc/json` files. The metadata addresses reflect the upstream format.

Shape and named-control provenance:

- [A-10C module](https://github.com/DCS-Skunkworks/dcs-bios/blob/11c6209beca4e5aae201a5cf62a7c346ebabbfc8/Scripts/DCS-BIOS/lib/modules/aircraft_modules/A-10C.lua): `UFC_MASTER_CAUTION`, `MASTER_CAUTION`, and A-10C/A-10C_2 aliases.
- [MetadataStart](https://github.com/DCS-Skunkworks/dcs-bios/blob/11c6209beca4e5aae201a5cf62a7c346ebabbfc8/Scripts/DCS-BIOS/lib/modules/common_modules/MetadataStart.lua) and [MetadataEnd](https://github.com/DCS-Skunkworks/dcs-bios/blob/11c6209beca4e5aae201a5cf62a7c346ebabbfc8/Scripts/DCS-BIOS/lib/modules/common_modules/MetadataEnd.lua): aircraft name and update-complete counter.
- [Developer guide](https://github.com/DCS-Skunkworks/dcs-bios/blob/11c6209beca4e5aae201a5cf62a7c346ebabbfc8/Scripts/DCS-BIOS/doc/developerguide.adoc): newline commands, UDP defaults, little-endian aligned writes and four-byte synchronization marker.
- [Documentation classes](https://github.com/DCS-Skunkworks/dcs-bios/tree/11c6209beca4e5aae201a5cf62a7c346ebabbfc8/Scripts/DCS-BIOS/lib/modules/documentation): catalogue field names.

The production loader requires `A-10C.json`, `AircraftAliases.json`,
`MetadataStart.json` and `MetadataEnd.json` in the selected installed `doc/json`
directory; legacy `MetaDataStart.json`/`MetaDataEnd.json` spelling also works.
It validates the named 0/1 `set_state` input, one-bit caution output, aligned
addresses, nonoverlapping metadata and the low-byte update counter. It supports
the A-10C aircraft aliases advertised by that installed catalogue.

Export writes remain pending until the metadata-end counter completes an update.
The parser handles split synchronization markers, headers, payloads and multiple
updates per datagram. Unknown bytes cannot stand in for LED state. The session
requires both a known aircraft name and freshly observed caution feedback after
a stream reset, stale export, backward counter or aircraft change. Advancing
counter-only updates keep an already confirmed, unchanged LED value fresh;
duplicate counters neither replace feedback nor refresh its age. An empty/NONE
aircraft name ends the mission, including the upstream final update whose counter
may repeat. Memory is bounded to the 16-bit address space and frame writes to
128 KiB. These tests also exercise malformed alignment, length, masks and strings.

The runtime must establish the selected button's current event baseline after
every route, stream or mission transition. A held button arms only after release;
old events never become new commands. A live press sends `UFC_MASTER_CAUTION 1`
and its release sends `UFC_MASTER_CAUTION 0`, each with a newline. Disarming or
stopping after an attempted press emits one neutral release. A failed UDP send is
explicitly uncertain: the press is never retried, the neutral release is attempted
once, and further input waits for an explicit stop/start. Successful send counts
prove only that the datagram was sent, not that DCS applied it.

LED state means confirmed device action state rather than electrical verification.
Stale export requests LED off. A lost setter reply requires readback before another
conditional setter; the session does not blindly replay that setter. Firmware
continues to advertise and drive its own physical or simulated LED, while Cockpit
owns the simulator mapping and UDP connection.

Run the native regression and per-module production coverage gate with
`python3 scripts/dcs-bios-coverage.py --output-dir ../.research/dcs-bios-coverage`.
Install Rust's `llvm-tools-preview` component first. The gate covers the catalogue,
parser/session, background runtime and panel event validators separately and
requires each module to exceed 70% executable-line coverage. Local UDP tests need
permission to bind loopback sockets in restricted execution environments.
