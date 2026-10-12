# Pico W gateway simulator

This firmware turns an **RP2040 Raspberry Pi Pico W** into a USB composite device: one CDC serial connection for Cockpit and a separate HID joystick interface for each registered module. Cockpit simulates CAN module inputs; the Pico publishes real USB HID reports. No Apple signing identity, GPIO wiring, or soldering is needed.

## Flash the built firmware

1. Use a **micro-USB data cable**. Unplug the Pico from the Mac.
2. Hold the Pico's **BOOTSEL** button while plugging it in. Release the button after the **RPI-RP2** drive appears in Finder.
3. Copy `build/osg-pico-gateway.uf2` onto **RPI-RP2**. Alternatively, use Cockpit's **Flashing instructions** page: in the desktop app, **Download gateway firmware** exports its bundled image to Downloads and shows the saved path. Copy that file onto RPI-RP2; downloading does not flash the Pico. Rebuild Cockpit after a new firmware build to refresh its bundled image.
4. The drive ejects and the Pico restarts automatically. Its normal firmware connection provides USB serial and four initial joystick interfaces.

Flashing replaces the firmware currently on the board. BOOTSEL is stored in ROM; hold it during connection to return to the flashing drive if recovery is needed. A BOOTSEL-mode board has no running gateway serial port. Use the RP2040 **Pico W**, not a Pico 2 W, with this build.

## Connect and test in Cockpit

From `cockpit/`:

```sh
pnpm install
pnpm dev
```

1. In the desktop window, select **Pico W · USB** as the gateway backend. The [desktop setup](../../README.md#run-the-desktop-app) lists Tauri/Rust build prerequisites. For the browser fallback, run `pnpm web:dev` and open `http://127.0.0.1:5173/`.
2. Refresh the USB ports, choose the Pico's `/dev/tty.usbmodem…` or `/dev/cu.usbmodem…` port, and click **Connect Pico**. The native service verifies the firmware identity and protocol before accepting the connection.
3. Click **Start gateway**. Select a module and open **Inputs**. Buttons toggle held states; sliders send unsigned 16-bit axes. Panel encoder directions are exposed as buttons. Physical USB HID applications can read these reports while Cockpit holds the separate CDC interface.
4. Open a setup session to edit a label, debounce, or inversion, then use **Apply & save**. End the setup session before changing the layout or disconnecting. HID input tests do not need a setup session.

The default inventory is a three-button shifter, three-axis pedal set, sixteen-button panel, and an offline one-axis handbrake. Offline modules keep their published interface and emit neutral inputs. **Stop gateway** also neutralizes reports while leaving the USB interfaces enumerated. **Disconnect Pico** stops the simulator and releases serial access. Unplugging, closing serial, closing Cockpit or losing its UI connection, or a ten-second firmware command timeout stops inputs; connect/start explicitly to resume.

## Add and remove modules

Use **Add device** to choose a type, unique stable module identity, name, button count, and axis count. Up to **eight modules**, each with **0–32 buttons** and **0–6 axes**, are supported; each module must have at least one input. Adding registers a pending module. Its HID controls become available after **Reconnect gateway**.

Use **Remove module** in the inspector to remove a registration. Its currently published inputs are neutralized immediately. Reconnect to remove its USB HID interface. Removing a pending module cancels its registration; removing the final module leaves a valid CDC-only gateway from which new modules can be added.

**Reconnect gateway** acknowledges the command, disconnects USB, rebuilds its descriptors, then reconnects with the new layout. Cockpit reopens the same board, verifies its identity, rediscovers the inventory, and releases old setup claims and pending input jobs. Click **Start gateway** afterward. The USB port path can change across re-enumeration. Check game bindings: logical module IDs stay stable, but compact USB interface numbers can change when modules are removed.

All interfaces share the Pico's board-specific USB serial, `OSG-PICO-…`. Firmware discovery exposes each logical module ID and its `hidInterface` number. The `OSG-SIM-…` module serial in the protocol is a logical identity, not an independent USB serial.

## Controller names

Default device names are **OSG Sequential shifter**, **OSG Pedal set**, **OSG Button panel**, and **OSG Handbrake**. New-module defaults also use the `OSG` prefix. A module's saved **Device name** is its controller display name.

Firmware **0.1.1** publishes that saved name as the HID interface's USB string. Renaming an already published interface requires **Reconnect gateway** to update the USB descriptor. Unicode labels are encoded as USB UTF-16. The composite board itself retains the shared product name `OpenSimGear Pico Gateway`.

macOS uses that shared product name for every HID interface even when USB interface strings differ. The desktop app therefore sets each actual HID registry service's `Product` property directly from Rust through IOKit, matching the connected board's VID/PID, joystick usage, USB serial, and interface number. Names are reapplied after connect, rename, and USB reconnection; this also works with firmware 0.1.0. Naming changes existing controllers and needs no virtual-HID entitlement, Apple signing identity, administrator launch, or runtime Swift compiler. Naming failures appear in Cockpit while USB input testing remains available. The explicit browser fallback retains `simulator/pico-hid-names.swift` and requires Apple's command-line Swift compiler; the independent macOS HID observer also uses those development tools.

Fresh IOHID enumeration sees the individual names. Apps that have already cached controllers may need to rescan or reopen; macOS HID event-service/GameController consumers can retain the shared product name. The override applies on this Mac while Cockpit is connected and is reapplied when Cockpit reconnects. Other computers see the firmware's USB strings according to their USB/HID driver.

Settings, inventory changes, and registration live in **RAM**. Power cycling or resetting the Pico restores the default inventory. Flash 0.1.1 to get the prefixed defaults after reset. CAN transport, physical sensors, flash persistence, calibration, and production firmware updates are outside this simulator.

## Rebuild

Install CMake, Git, a C compiler for host tests, and the Arm GNU embedded toolchain (`arm-none-eabi-gcc`). The Raspberry Pi [C/C++ SDK](https://github.com/raspberrypi/pico-sdk) is pinned to **2.3.1**; its TinyUSB submodule is pinned by that SDK release. From the parent workspace containing `cockpit/`:

```sh
mkdir -p .research
git clone --depth 1 --branch 2.3.1 https://github.com/raspberrypi/pico-sdk.git .research/pico-sdk
git -C .research/pico-sdk submodule update --init --depth 1 lib/tinyusb
cd cockpit
pnpm pico:build
```

For a SDK elsewhere, set `PICO_SDK_PATH` to its absolute directory. If tools are not on PATH, set `ARM_GNU_BIN` to the directory containing `arm-none-eabi-gcc` and `OSG_CMAKE` to your CMake executable. The build selects `PICO_BOARD=pico_w`, disables wireless, and writes the UF2 and SHA-256 checksum to `simulator/pico/build/` and `public/firmware/` for local download. No build command flashes a connected board.

```sh
pnpm pico:test
pnpm native:test
pnpm transport:test
pnpm check
pnpm build
pnpm simulator:verify --watch --timeout 20
```

`pico:test` runs host C protocol/lifecycle/report/descriptor tests with address and undefined-behavior sanitizers, UF2 conversion tests, browser transport tests, and bridge-to-real-C-protocol integration tests. `native:test` checks the Rust serial service separately; `transport:test` checks UI IPC routing and the browser fallback. Mock serial transport tests do **not** establish physical USB enumeration or report delivery. The independent macOS IOHID verifier observes actual OS devices and reports, including Pico joystick interfaces.

## Wire protocol and identifiers

CDC carries bounded UTF-8 newline-delimited JSON. Requests contain `id` and `command`; commands include `hello`, `status`, `start`, `stop`, `input`, `presence`, `configure`, `add`, `register`, `remove`, and `reconnect`. Responses echo `id`, contain `ok` and the authoritative `state`, and include an `error` on rejection. Protocol version is **1**. Configuration ownership remains local to the Cockpit native service (or browser bridge fallback); this is a simulator protocol rather than the finalized OpenSimGear production protocol.

HID descriptors use joystick application collections without report IDs. Reports contain packed buttons with byte padding followed by unsigned little-endian 16-bit X/Y/Z/Rx/Ry/Rz axes. Inversion applies to axes, or swaps the first and third shifter buttons. Debounce stages button changes while axes update immediately.

VID `0xcafe`, PID `0x4010` are **experimental example identifiers for local development**, not an OpenSimGear allocation. Do not distribute products with these identifiers. The Pico SDK and TinyUSB retain their upstream licenses; this repository's own licensing is described in the root README.

## Hardware acceptance check

The historical browser prototype and Node bridge were tested on **2026-10-12** against a physical Pico W (`OSG-PICO-E661640843565431`) using the independent macOS IOHID observer. The board exposed four separate joystick interfaces alongside CDC. Shifter buttons, panel buttons 1 and 16, all three pedal axes, and the handbrake axis produced the expected raw reports and macOS element values. Bringing the handbrake offline cleared its held axis while retaining its interface.

A temporary module with four buttons and one axis was added through Cockpit. Explicit USB reconnection changed the host inventory from **four to five controllers**; its button 4 and full-scale axis arrived as report `08ffff`. Removing the held module immediately sent `000000`, and another reconnection restored **four controllers**. Cockpit recovered the serial connection after both layout changes. Stopping the gateway also released a held shifter button. The temporary module was removed, the original inventory restored, and the gateway restarted with neutral inputs. Raw observation logs and screenshots are saved locally in `output/pico-hardware/`.

Firmware **0.1.1** was flashed to the same Pico through its `RPI-RP2` BOOTSEL drive and verified after the automatic firmware reboot. Before Cockpit connected, an independent USB registry snapshot showed all four native interface strings already named `OSG Sequential shifter`, `OSG Pedal set`, `OSG Button panel`, and `OSG Handbrake`. Cockpit then reported version 0.1.1 and the matching default names, and its browser bridge’s Swift macOS helper applied those names to all four HID services. A fresh HID reader confirmed them and observed a shifter press/release as `01 → 00`, with Button 1 returning to zero. Evidence for this flash and automatic reset is in `output/pico-hardware/firmware-reset/`; this check did not include a separate power cycle.

The **native Tauri app** was also tested on the same day with separate IOHID observation. Rust serial communication and Rust IOKit naming passed: shifter inputs, all three pedal axes at 32768, panel button 16, a temporary four-button/one-axis module, and real **four → five → four** controller enumeration. Removing held inputs, stopping the gateway, and quitting the app while holding a button produced neutral OS reports; app quit closed CDC. The final standalone release app delivered shifter inputs with its development server stopped. Its flashing guide exported the exact 92,160-byte bundled UF2 to Downloads. See the [native acceptance and build/export report](../../docs/verification/2026-10-12-tauri-pico.md).

For a complete acceptance pass on another board, repeat these checks. Inversion, a CDC-only empty inventory, serial timeout, and physical unplug/replug recovery remain separate hardware checks beyond the sessions above:

1. Confirm four independent joystick interfaces and one serial interface in the independent HID observer.
2. Exercise shifter/panel buttons and every pedal/handbrake axis; confirm raw reports and parsed values, including inversion and neutral reports.
3. Add a module, reconnect, and confirm a new joystick interface. Remove it, reconnect, and confirm its interface disappears.
4. Remove all modules, reconnect, and confirm CDC remains usable. Add a new module and repeat.
5. Disconnect a module, stop the gateway, close serial, and unplug/replug; confirm neutralization and explicit restart behavior.

Raspberry Pi's [Pico W datasheet](https://datasheets.raspberrypi.com/picow/pico-w-datasheet.pdf) documents native USB and BOOTSEL. [TinyUSB](https://docs.tinyusb.org/en/latest/) provides the RP2040 USB device stack.
