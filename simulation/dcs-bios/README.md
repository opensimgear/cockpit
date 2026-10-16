# Simulated DCS-BIOS panels

## Development status and disclaimer

This project is heavily under development and is not ready for use. Use it entirely at your own risk. To the fullest extent permitted by applicable law, the authors and contributors disclaim all liability for any damage, loss, or other issues arising from its use.

This local peer sends the DCS-BIOS binary export format and receives named
panel commands. It is for software development without DCS or hardware.
Its authored catalogue uses synthetic addresses; Cockpit resolves them
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

The radio fixture uses `device-template/simulation/configs/radio-panel.json`.
It advertises two encoders, two maintained switches, one momentary button, one
calibrated axis, the board status LED and three output ports: a 7-cell ASCII
frequency display, a 2-cell ASCII preset display and a 16-cell UTF-8 text display
with a 63-byte limit. The current starter board has no fitted LCD and advertises
only its actual LED; a fitted display driver supplies the same production port
callbacks used by these native buffers. Device `sim <id> turn <ordinal> <detents>`
visits sampled quadrature edges at 2 ms intervals. `display_delay <port> <ms>`
models a synchronous native driver delay from 0 to 250 ms.

In Cockpit's DCS-BIOS profile editor, bind the panel's encoder 0 to
`UHF_PRESET_SEL` using `fixed_step`, its switch 0 to `UHF_SQUELCH`, its button 2
to `UHF_TEST`, and its axis 0 to `UHF_VOL` using `set_state`. A second encoder
can control `UHF_10MHZ_SEL` using `fixed_step`; `UHF_VOL` also offers
`variable_step`. Bind `UHF_FREQUENCY` and `UHF_PRESET` to the corresponding
display setters. An integer such as `UHF_VOL` can be formatted into the UTF-8
text port with scale `100 / 65535`, one decimal place and a `VOL ` prefix.
Every input command has one owner in a profile. A momentary button held at
Start must release before it can send a fresh press; maintained switches and
available axes synchronize their current absolute state at each fresh baseline.

The following names and interfaces match DCS-BIOS's A-10C module. The addresses
below are authored fixture values and must never be copied into a live profile:

| Control | Interface/output | Synthetic address |
| --- | --- | --- |
| `MASTER_CAUTION` | integer boolean | `0x1240`, mask 4 |
| `UFC_MASTER_CAUTION` | `set_state`, max 1 | command only |
| `UHF_FREQUENCY` | string, 7 bytes | `0x1260` |
| `UHF_PRESET` | string, 2 bytes | `0x1268` |
| `UHF_VOL` | `set_state`/`variable_step`, max 65535; integer output | `0x1270` |
| `UHF_PRESET_SEL` | `set_state`, max 20; `fixed_step`; integer output | `0x1272` |
| `UHF_10MHZ_SEL` | `set_state`, max 9; `fixed_step`; integer output | `0x1274` |
| `UHF_SQUELCH` | `set_state`, max 1; integer output | `0x1276` |
| `UHF_TEST` | `set_state`, max 1; integer output | `0x1278` |

Radio controls and export faults are also available on stdin:

```text
sim 11 frequency 123.450
sim 12 preset 20
sim 13 volume 32768
sim 14 half_frame 1
sim 15 complete
sim 16 half_frame 0
sim 17 frequency_hex ff
sim 18 malformed 1
sim 19 malformed 0
sim 20 coalesce 4
sim 21 rate 200
sim 22 flood 1
sim 23 flood 0
```

`half_frame` sends all fields but holds the final update-counter block;
`complete` commits that frame. No new frame is sent while one is held.
`frequency_hex` can export invalid UTF-8 for unknown-value tests. `malformed`
without an argument sends one malformed frame, while its boolean form repeats
or stops that fault. `coalesce` sends 1–32 complete frames in one datagram.
`rate` selects 1–1000 exports per second; `flood` changes frequency, preset and
volume each frame. Commands received over UDP are validated against this
catalogue, with at most 256 retained records. Each record includes a monotonic
sequence and wall-clock receive time for external physical-to-peer latency
measurements.

The radio acceptance test runs two real native gateways and panels with the
same module identity, private discovery and UDP ports. It checks cursor scope,
atomic full-width strings, encoding bounds, calibrated acquisition while HID
is disabled, stale fallback, per-gateway isolation and readable reconciliation
after a lost setter acknowledgement. A 200 Hz export flood and a 40 ms native
display callback delay exercise latest-value queues and input responsiveness.
`radio-performance.json` records queue high-water marks, coalesced writes,
action latency, throughput, maximum input poll interval and external input p95.
The target of p95 at most 250 ms qualifies fast native simulation only; live DCS
and physical hardware remain deferred.
