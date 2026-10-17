# Native clock fixture corrections

The `348fba2` startup correction makes native phases start at their actual captured clock. The resulting CI failures expose three test assumptions that still use zero or a synthetic timestamp of 100.

The applied-port GatewayManager test now requires a positive captured timestamp and verifies that the exact applied sample and timestamp survive saving a newer draft. The applied revision, value, owner validation and Stop assertions remain intact. This test is compiled locally but runs only in CI, because the local Cockpit service must remain the sole GatewayManager.

Two positive action Host fixtures now stamp their manually dispatched post-Start effects with the current reducer clock. The acquisition test requires an empty dispatch diagnostic list before waiting for Invoke. The endpoint-failure test requires an actual Invoke and the intended `failed invoke` diagnostic, so an invalid timestamp cannot satisfy the degradation assertion. Deliberately rejected pre-Start effects, sequence numbers, lease counts, deadlines and production guards are unchanged.

The old acquisition fixture fails immediately when the reducer clock is 18,669 and its synthetic effect clock is 100. The corrected copied native test executable passes 11 action Host, eight companion Host and 22 publication tests; two hardware-dependent companion tests remain ignored. All 385 recorded source hashes remain unchanged throughout this run. Compilation and three safe inspection tests also pass. The actual GatewayManager assertion and complete CI suites still require a fresh CI result.

Private evidence is retained in `.research/p9-action-clock-fixture/final-manifest.json` and `.research/p9-inspection-capture-clock/final-manifest.json`. These changes affect tests only. The active physical qualification continues to use the reviewed `348fba2` executable; no qualification flag or runtime clock behavior changes here.
