# STM32 gateway protocol fixtures

`stm32-gateway-v2.json` is the gateway C formatter output, with LF line endings, on
2026-10-12. The canonical fixture belongs to the OpenSimGear protocol repository's
`test-vectors/` directory. This checked-in copy keeps Cockpit unit tests independent
of another checkout. To regenerate after running the E2E dependency bootstrap:

```sh
cmake -S .e2e/deps/device-gateway/firmware -B .e2e/build/gateway-vectors
cmake --build .e2e/build/gateway-vectors
.e2e/build/gateway-vectors/osg_gateway_test --cockpit-vectors > src-tauri/tests/fixtures/stm32-gateway-v2.json
```

The console remains CDC v2 and now reports `canProtocol=3`
and firmware `0.3.0` for the Cyphal gateway. It includes discovery, pending and active profiles, maximum
generic capabilities, direct USB ownership, offline slots, stale epochs and
profile changes. `stm32::tests::accepts_actual_c_firmware_formatter_conformance_vectors`
validates and decodes these actual C outputs. Update both copies together after
changing the wire format; handcrafted mock serial states supplement these
fixtures for transport failures and registry lifecycle tests.
