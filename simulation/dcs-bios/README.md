# Simulated DCS-BIOS master caution

This local peer sends the DCS-BIOS binary export format and receives named
master-caution commands. It is for software development without DCS or hardware.
Its minimal authored catalogue uses synthetic addresses; Cockpit resolves them
from JSON in the same way as an installed DCS-BIOS catalogue.

Run ordinary Cockpit, a gateway from `device-gateway/simulation`, and the
`device-template/simulation/configs/master-caution.json` panel from its owning
repository. Use one private `OSG_SIMULATION_NAMESPACE` for all three when testing
alongside an existing Cockpit. The panel exposes D0 as logical button 0 and the
same `led.status.set_mode` action as the current board. Simulation processes do
not create OS HID controllers.

Start this peer separately from Cockpit:

```sh
node simulation/dcs-bios/peer.mjs --command-port 27778 --export-port 25010
```

In Cockpit, select the panel's **DCS-BIOS** tab. Disable its HID controller using
the existing confirmation, wait for the layout to settle, and disconnect direct
USB input. Enter this repository's absolute `simulation/dcs-bios/catalogue`
directory, button 0, and `led.status.set_mode`. Set command host and export
listen address to `127.0.0.1`, command port 27778, export port 25010, and clear
the multicast group for this unicast peer. Save, then Start. Save alone never
starts a binding; restarting Cockpit also requires explicit Start.

The peer accepts these lines on stdin and emits JSON responses:

```text
sim 1 warning 1
sim 2 inspect
sim 3 pause 1
sim 4 pause 0
sim 5 aircraft F-16C_50
sim 6 aircraft A-10C_2
sim 7 fragment 1
sim 8 freeze 1
sim 9 freeze 0
sim 10 aircraft NONE
```

`warning 1` exports master caution on. A production panel `sim <id> contact 0 1`
followed by `contact 0 0` sends `UFC_MASTER_CAUTION 1` and `0`; the peer clears
its warning on the acknowledgement press. `pause` stops export, `freeze` repeats
an update counter, and `fragment` splits each frame across UDP datagrams. Both
paused export and frozen counters eventually select Cockpit's LED-off fallback.
Unsupported aircraft and mission end also select off. The peer retains at most
256 received commands for inspection. Stop sends one neutral release if this
binding forwarded a held press, then requests LED off.

For a real DCS-BIOS installation, use its own `doc/json` directory and the
default transport settings shown in Cockpit. The installed catalogue must
include `A-10C.json`, `AircraftAliases.json`, `MetadataStart.json` and
`MetadataEnd.json` (legacy `MetaDataStart`/`MetaDataEnd` filenames are accepted).
This milestone verifies the software peer and production panel contracts;
live DCS and hardware qualification are deferred.

`pnpm e2e` provisions production firmware simulations and exercises this peer
automatically. See the [verification record](../../docs/verification/2026-10-13-dcs-bios-master-caution.md)
and [official DCS-BIOS developer guide](https://github.com/DCS-Skunkworks/dcs-bios/blob/main/Scripts/DCS-BIOS/doc/developerguide.adoc)
for the wire format and software evidence.
