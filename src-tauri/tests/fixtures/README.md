# STM32 gateway protocol fixtures

`x-plane-11-rref.json` and `x-plane-12-rref.json` are authored C172/Baron
software fixtures, not simulator captures. Provider tests encode their values as
RREF replies and exercise the production parser and channel mapping. Real
simulator, Windows native acquisition and physical hardware qualification remain
pending; see [the X-Plane guide](../../../docs/x-plane.md).

`stm32-gateway-v3.json` is the current gateway C formatter output for the automatic
controller lifecycle. `stm32-gateway-v2.json` retains the older manual contract.
The canonical fixtures belong to the OpenSimGear protocol repository's
`test-vectors/` directory. This checked-in copy keeps Cockpit unit tests independent
of another checkout. To regenerate after running the E2E dependency bootstrap:

```sh
cmake -S .e2e/deps/device-gateway/firmware -B .e2e/build/gateway-vectors
cmake --build .e2e/build/gateway-vectors
.e2e/build/gateway-vectors/osg_gateway_test --cockpit-vectors > src-tauri/tests/fixtures/stm32-gateway-v3.json
```

CDC v3 reports `canProtocol=3`, automatic lifecycle, declared discovered/remembered/
controller capacities, explicit enabled/published flags, descriptor positions and
report IDs, frozen profile compatibility, layout generations and admission errors.
Its vectors cover enrollment, pending/active profiles, generic maximum capabilities,
direct USB ownership, offline ghosts, stale epochs, Disable and pending Enable.
`stm32::tests::accepts_actual_c_automatic_lifecycle_formatter_vectors` validates those
actual C outputs; the older formatter test checks v2 independently. Update the
canonical and Cockpit copies together after
changing the wire format; handcrafted mock serial states supplement these
fixtures for transport failures and registry lifecycle tests.
