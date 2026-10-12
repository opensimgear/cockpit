# STM32 gateway protocol fixtures

`stm32-gateway-v2.json` is the unmodified output of the gateway C formatter on
2026-10-12, generated from the parent OpenSimGear workspace with:

```sh
cmake -S device-gateway/firmware -B .research/gateway-native
cmake --build .research/gateway-native
.research/gateway-native/osg_gateway_test --cockpit-vectors > stm32-gateway-v2.json
```

The canonical fixture lives in the adjacent OpenSimGear protocol repository's
`test-vectors/` directory. This copy makes Cockpit's Rust tests independent of an
adjacent checkout. It includes discovery, pending and active profiles, maximum
generic capabilities, direct USB ownership, offline slots, stale epochs and
profile changes. `stm32::tests::accepts_actual_c_firmware_formatter_conformance_vectors`
validates and decodes these actual C outputs. Update both copies together after
changing the wire format; handcrafted mock serial states supplement these
fixtures for transport failures and registry lifecycle tests.
