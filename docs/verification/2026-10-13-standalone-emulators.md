# Independent gateway/device emulator acceptance — 2026-10-13

> Historical acceptance record. This describes the implementation at the time of the run. The repository-owned simulation migration superseded the commands and Cockpit simulation controls below; those commands are retained as evidence and are not current setup instructions. Use the [current Cockpit guide](../../README.md) and [repository-owned E2E bootstrap](../../tests/e2e/README.md).

This work-in-progress flow runs the gateway and each device as independent native
firmware processes. Ordinary Cockpit discovers a local gateway through its normal
Rust transport, validates the production CDC identity/protocol/epoch handshake,
and receives devices discovered by that gateway through real libcanard CAN frames.
Cockpit does not launch firmware processes or use an emulator mode.

## Process and transport behavior

The gateway advertises bounded metadata in the current user's private
`/tmp/opensimgear-<user-id>/gateways/` directory. Its loopback stream carries the
same CDC commands as the physical STM32 gateway. Device processes exchange
serialized CAN identifiers, flags and payload bytes through a separate loopback
UDP broker. Application settings and descriptor behavior remain in the production
gateway/module C implementations.

Independent processes use monotonic wall time. Device discovery, leases, report
delivery and offline deadlines keep progressing without a connected Cockpit or
terminal request. Each role owns a locked persistent flash directory; gateway
registration, device settings and boot counters survive process restart. Duplicate
live identities and concurrent access to the same flash state are rejected.

Gateway layout Apply drains its CDC reply, detaches for the production 250 ms gap,
and permits normal reconnection by UID. A new gateway process can advertise a
different stream port; Cockpit rediscovers the current endpoint before reconnecting.
Each new Cockpit adapter seeds a fresh request counter to avoid collisions with a
still-running module's completed-request cache.

The standalone gateway uses a dynamic loopback TCP listener. Generic native
discovery also retains private Unix stream support. Repeated backpressure/restart
checks exposed intermittent timeouts with Unix streams on this macOS host; the
standalone process uses TCP while preserving the CDC protocol and normal discovery.

## Native desktop acceptance

The macOS debug app was packaged with bundled frontend assets under the workspace
`.research/` directory and launched through its `.app` path. No development server
or emulator executable environment override was used. Gateway and device processes
were launched separately from terminals.

The observed native UI flow covers:

1. Start the gateway first. Cockpit discovers and connects to it with an empty
   device inventory.
2. Start a device independently. It appears online in Cockpit's gateway inventory.
3. Open a setup session, save `Standalone native é`, add a second axis, and set the
   first axis to raw range 100–3900, inversion and deadzone 1000. Apply & save shows
   active/saved generation 2, eight buttons, one hat and a six-byte report. The
   production module journal records the same configuration.
4. End setup, Add controller and Apply layout. Cockpit reconnects by gateway UID
   and shows the matching registered profile and generated report ID.
5. Inject contact 1 and ADC samples 100/4095 through the device terminal. Cockpit's
   read-only input view displays buttons 2/7 on and both axes at 100%, with a neutral
   hat. Module and gateway packed reports agree: `4208ffffffff`. The gateway has
   report ID 1 and records one explicit layout reconnect.
6. Close Cockpit. Both firmware processes remain running. Reopen Cockpit and
   recover the registered device and saved configuration through normal discovery.

Native accessibility observations and firmware-state evidence were captured during
the run; a result record is retained locally at
`.research/standalone-ui-final/result.json`. The automated scenario below is
maintained in the repository.

## Automated acceptance

The direct C daemon acceptance runs two modules, device-before-gateway launch,
identity admission, real CAN node collision repair, configuration, generated HID
descriptors and packed inputs. It checks unread-client backpressure, offline
neutralization, shared-flash exclusion, saved settings and controller registration
across crashed-process restarts, and firmware lifetimes after client closure.

The Rust-to-C test discovers an independently launched gateway with no fabricated
device, then connects a separate device and saves a Unicode name with buttons,
two calibrated axes, a hat, an encoder and a selector. It explicitly registers and
applies the layout, compares module/gateway packed reports, kills/restarts device
and gateway processes, and checks saved settings, fresh boot/epoch and registration
recovery. A killed Cockpit service retains the firmware writer lease; a new service
gets the expected ownership error promptly, then claims and reads settings after
release. Closing the service leaves both firmware processes alive.

All firmware emulator checks use AddressSanitizer and UndefinedBehaviorSanitizer.
The combined deterministic fixture remains covered by its five existing acceptance
flows and the legacy Rust-to-C transaction test. Frontend transport checks and
Svelte diagnostics also run against the updated normal browser bridge.

| Check | Result |
| --- | --- |
| Packaged native macOS desktop scenario | Passed; saved generation 2, live inputs, registration and reopen |
| Actual Rust → independent gateway/device acceptance | Passed, including dynamic-port restart and fresh app session ownership |
| Direct independent C acceptance | 12 consecutive backpressure/restart runs passed with ASan/UBSan |
| Legacy combined C acceptance | Five flows passed with ASan/UBSan |
| Rust library tests | 44 passed; includes real local stream IO, timeout and metadata/port validation |
| Frontend transport/configuration tests | 30 passed |
| Generic browser transport bridge | Inventory, origin restriction and service crash recovery passed |
| Svelte diagnostics | Zero errors and warnings |
| Frontend and native app packaging | Passed; bundled macOS debug app |

## Reproduce

From the parent workspace with matching adjacent firmware/protocol checkouts:

```sh
pnpm --dir cockpit emulator:build
CARGO_TARGET_DIR="$PWD/.research/cockpit-native-target" cargo build \
  --manifest-path cockpit/src-tauri/Cargo.toml --bin osg-gateway-service
pnpm --dir cockpit emulator:test
pnpm --dir cockpit native:standalone:test
```

For manual use, run each command in a separate terminal:

```sh
pnpm --dir cockpit emulator:gateway
CARGO_TARGET_DIR="$PWD/.research/cockpit-native-target" pnpm --dir cockpit dev
pnpm --dir cockpit emulator:device
```

Start the desired gateway before Cockpit when physical gateways are also attached.
Additional devices need unique identities. Use separate `--state-dir` paths under
`.research/` for isolated runs. The then-current `simulator/native-gateway/README.md`
documents terminal input controls and the optional `web:native` browser transport.

## Boundary

The local stream replaces USB CDC and the loopback broker replaces the CAN wire.
HID submission/completion and flash operations are simulated. This establishes
the production configuration, descriptor and lifecycle flow through Cockpit; it
does not establish OS USB/HID enumeration, STM32 HAL timing, electrical CAN or
physical brownout behavior. No new firmware/app version or release was created.
