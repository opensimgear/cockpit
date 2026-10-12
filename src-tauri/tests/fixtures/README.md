# STM32 gateway protocol fixtures

`stm32-gateway-v2.json` is the gateway C formatter output, with LF line endings, on
2026-10-12, generated from the parent OpenSimGear workspace with:

```sh
cmake -S device-gateway/firmware -B .research/gateway-native
cmake --build .research/gateway-native
.research/gateway-native/osg_gateway_test --cockpit-vectors > stm32-gateway-v2.json
```

The canonical fixture lives in the adjacent OpenSimGear protocol repository's
`test-vectors/` directory. This copy makes Cockpit's Rust tests independent of an
adjacent checkout. The console remains CDC v2 and now reports `canProtocol=3`
and firmware `0.3.0` for the Cyphal gateway. It includes discovery, pending and active profiles, maximum
generic capabilities, direct USB ownership, offline slots, stale epochs and
profile changes. `stm32::tests::accepts_actual_c_firmware_formatter_conformance_vectors`
validates and decodes these actual C outputs. Update both copies together after
changing the wire format; handcrafted mock serial states supplement these
fixtures for transport failures and registry lifecycle tests.
