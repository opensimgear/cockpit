# MCP slow observer isolation

The running Windows dev Cockpit drove a private vJoy slot from an authored
debug controller. One authenticated MCP HTTP observation reader stopped
reading after its initial snapshot. A second reader consumed each requested
sample and the Stop retirement while the first was still unread.

- The slow reader stayed unread for 1,956 ms. During that interval the attached
  137-control VKBsim Gladiator EVO R advanced from report 4463 to 4487 in the
  same native generation.
- The authored vJoy button remained held for at least 750 ms. Its profile
  stayed active and the exact slot stayed busy beyond the 250 ms feeder lease.
  Windows Raw Input saw press, release, press, then the release produced by Stop.
- MCP Stop replied in about 68 ms in this run. The owner retired and the exact
  slot returned to free before the slow stream resumed and received loss.
  A separate portal Stop run also released the held button. Both fixtures
  cleaned up the temporary profile, and the MCP fixture revoked its credential
  and restored the listener setting.
- The delayed physical preview check passed separately. Native HTTP stress
  tests in the 2,352-test suite covered full subscription capacity, independent
  desktop and MCP delivery, Stop admission and explicit overflow/rebaseline.

The actual app run kept the HTTP body unread. The native stress fixtures
exercise subscription capacity and overflow separately. The 68 ms figure
measures one Stop RPC and does not measure physical motion or display latency.
The virtual transitions were authored, and no game receipt was measured.

Executable SHA-256:
`80b4fe17794f8eab3dfa5f63296fee968a007a9976bf90b20426d5271f8dc99f`.
Native source commit: `ac9bf7c8d5e4587f03852d96dbe1ab622a67f0e7`.
Evidence: `.research/reactive/mcp-vjoy-held-stop.json`.
