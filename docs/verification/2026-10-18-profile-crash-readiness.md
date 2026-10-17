# Reconnect readiness before the active-host crash case

Exact head `e5ebabf9a482d2821e895c9b071261ab2c54b0bb`, Checks run `39072861605`, Linux native job `117514205378` completed the first production-C setter readback, Pause/Stop restoration and graceful native-service restart. The second Apply, before the active-process SIGKILL segment, failed at `profile-acceptance.mjs:104` with “This physical output has no ready advertised action adapter.” Five of six service E2E cases passed; the crash segment was not reached.

The retained inventory contains the same resource UUID and complete action contracts, with “Waiting for the complete current control and action capability snapshot.” The collector publishes current ports only after both event capability (including explicit unsupported) and ready actions match the current route/configuration. Missing capability reads are asynchronous. The retained evidence cannot identify which capability cache was incomplete.

The fixture already waits for a current native action catalogue before its first Apply. Its reconnect path now uses the same existing bounded passive wait before the second, single Apply. It requires the original resource/action/contract hash, a complete current catalogue, one writable CAN connection and the exact captured gateway/module/node/epoch/boot/lease/configuration identity. It does not replace assignments, retry Apply, increase timeouts or change native admission.

Local validation: Node syntax and five pure readiness regressions cover stale-to-current publication, partial catalogues, wrong identities/contracts/routes, unavailable metadata and propagated native read errors. No local service, Manager or device operation was run. The corrected production-C service E2E and actual SIGKILL/recovery remain CI-pending.

Private retained evidence: `.research/p9-final-e5ebabf/linux-native-job.log` and `integration-evidence.zip`, artifact `12116004118`, ZIP SHA-256 `1e1ef82c12c461161acac0c1f518180a4d1a4e1c731cd4adf9add16dd20ef604`.
