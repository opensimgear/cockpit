# Unified Profile acceptance verification

The P9 acceptance cutover uses unified Profile schema2 documents and native resource/port pins. Independent mapping, DCS, integration, game-profile and visibility starts are negative boundaries. Existing device configuration/manual action scenarios, private feeder startup checks, WSS pairing/framing/isolation, and all qualification gates remain in place.

The native service scenarios run on isolated CI hosts. Running another gateway service beside the Cockpit development desktop would create a second native discovery/hardware owner, so local verification deliberately did not start it. CI exercises the actual Profile Host with production-C gateway/peripheral processes, including owned Setter output, competing writer rejection, atomically captured prior-state restoration on Pause/Stop, no persistent configuration writes and restart without automatic output activation.

The initiating E2E manifest explicitly pins firmware-common `ac2bacd2383ad2f9b720c2fc96268e9da9bf8d66`. Application refs are unchanged. The bootstrap requires their declared common baselines to agree, resolves the initiating manifest's full common SHA, and records declared and resolved revisions separately. A local-Git regression verifies the actual selected checkout contents, not only the resolver argument.

On 2026-10-17 an independent output-only production-C device simulator was built with `-fsanitize=address,undefined` against that exact common commit. It ran alone in a private simulation namespace with private Linux temporary flash state; no native service or gateway process was started. Actual readback verified:

| Operation | Observed result |
| --- | --- |
| Seed RGB `112233`, atomic `claim_current` | Actual prior state captured without applying an output |
| Owned Setter `445566` | Device readback `445566` |
| Duplicate same-owner claim | Same lease; original fallback retained |
| Competing owner and unowned write | `outputBusy` |
| Release | Captured `112233` restored |
| Separate 250 ms claim, owned `556677`, advance 251 ms | Captured `223344` restored by expiry |
| Flash counters, persistent configuration and descriptor | Unchanged |

ASan/UBSan stderr was empty. Local evidence is retained under ignored `.research/p9-native-retirement/c-simulation/`. This is production-C software simulation evidence. It does not prove OS USB enumeration, electrical behavior, game delivery or virtual/script/HidHide qualification.

Pure validation commands require no native service:

```sh
python -m unittest discover -s scripts -p 'test_*acceptance.py' -v
python -m unittest discover -s tests/e2e -p test_bootstrap.py -k CommonPinTests -v
node --test tests/e2e/profile-graph-fixtures.test.mjs
```

The graph builders were additionally parsed by the real pure `osg-profiles` schema/assignment validator, including the physical Setter fixture. Native integration and DCS coverage runners retain their full suite and per-module thresholds and pin native/shared sources, tests, manifests and locks throughout measurement. Simulator-owned process evidence remains separate from real platform qualification.
