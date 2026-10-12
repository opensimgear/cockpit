# Native gateway and module firmware emulator

This executable replaces physical CAN, USB and flash backends around production
C. It links the gateway CDC parser/formatter, gateway registry/descriptor/layout,
module runtime settings and acquisition, both flash journals, generated DSDL and
the real libcanard transport. Commands traverse simulated CAN frames; no second
configuration or descriptor model is maintained in JavaScript.

From the parent workspace containing `cockpit/`, `device-gateway/`,
`device-template/` and `protocol/`:

```sh
python3 cockpit/simulator/native-gateway/build.py --sanitize
python3 cockpit/simulator/native-gateway/test_native.py
```

Build output lives in `.research/native-gateway/native-gateway`. The build uses
the existing pinned firmware-common checkout under
`.research/generic-pinned-arm-example/firmware_common-src`; pass `--common-source`
for another matching local checkout. It never fetches dependencies.

Cockpit's debug Rust serial factory selects the executable with
`OSG_NATIVE_EMULATOR=/absolute/path/to/native-gateway`. The gateway identity is
`00112233445566778899aabb`, the module identity is
`00112233445566778899aabbccddeeff`, and initial boot/epoch are 1/65536. The process
accepts ordinary newline-delimited gateway CDC commands and emits the actual
correlated firmware responses. Each serial command advances the virtual clock;
firmware deadlines and ownership leases use that clock.

By default, independent emulator processes start with erased flash and commit
factory settings as active/saved generation 1 before serving inputs. Set
`OSG_NATIVE_STATE_DIR` to an isolated directory under `.research/` to retain
registry, settings and boot-counter pages across process restart. Flash erase
and programming obey 1-to-0 semantics. A failed operation partially erases or
programs the inactive page, allowing the production journal's commit/CRC
recovery to be exercised. Set `OSG_NATIVE_INITIAL_SETTINGS_FAULT` to an
operation index to interrupt that first baseline commit at startup; a subsequent
process restores only a recognizable interrupted deterministic baseline. Other
corrupt nonblank settings fail closed.

Debug administration uses `emulator <id> <operation>` and replies with
`{id,ok,error,emulator}`. These commands are emulator-only:

| Operation | Behavior |
| --- | --- |
| `inspect` | Descriptor bytes, packed reports, active settings, generations, reconnects, CAN/ACK and flash counters |
| `reboot module`, `reboot gateway` | Restore committed journals and advance the persisted boot/epoch |
| `fault module N`, `fault gateway N` | Interrupt erase/program operation N of the next write; 0 fails the next operation |
| `axis channel raw` | Set an ADC sample 0–4095 and acquire it |
| `contact input closed` | Set an active-low digital/expander contact; closed is 0/1 |
| `direct 0/1` | Release/take direct module USB input ownership |
| `disconnect 0/1` | Reconnect/disconnect gateway USB delivery |
| `advance milliseconds` | Advance up to 60000 ms for expiry/freshness tests |
| `drop_reply 0/1` | Deliver/discard configuration reply chunks |
| `capture_input`, `stale_input` | Retain/replay an old input transfer against the current route |
| `capture_delivery`, `complete_delivery` | Retain/complete a USB submission against the current layout/run generation |
| `probe_config source/boot/lease/epoch/node/identity/reordered` | Send a real CAN configuration fragment with an invalid target context |
| `direct_config <child>` | Call the production module CDC configuration handler to simulate an external direct writer |

Acceptance covers draft ownership and expiry, editable controls, UTF-8 names,
minimum/maximum calibration and inversion, invalid-profile rejection,
Apply/Save, unchanged descriptors while drafting, explicit gateway reconnect,
stale delivery rejection, direct ownership, volatile and saved reboot,
interrupted persistence and first user saves, interrupted factory initialization,
external direct writer freshness, cached reply replay, bounded reply loss, full
128-button/8-axis/4-hat configuration and process restart. Descriptor lengths
are checked by an independent HID item parser.

USB submission/completion is simulated. This emulator does not enumerate an OS
USB device; STM32 HAL timing, actual HID/CDC enumeration, electrical CAN and
physical power-loss behavior remain board tests.
