# Concurrent profile path and inspector readers

A 32-node path and a 16-node inspector with no shared topics exceeded the
previous browser context and native delivery worker limits. The expanded
fixture passed in the Linux WebKitGTK desktop and in Edge against the rebuilt
Linux and Windows browser-dev gateways, and in the Windows WebView2 desktop.
It switched to a second disjoint 32-node path and back,
observed true and false on both the original path and the inspector, and fenced
the retired owner after Stop. A second browser tab followed the owner through
Stop and reapply. The fixture used one inspection request and three topology
page requests.

All 57 focused JavaScript tests, 61 stream crate tests and 20 focused native
observation tests passed. Changed JavaScript transport modules reached
98.28–100% lines, 87.57–92.96% branches and 91.30–100% functions. The native
observation coverage gate passed with 88.89% runtime line coverage and
90.41% stdout delivery line coverage. The stream registry reached 98.15%
line coverage and 94.93% region coverage.

The locked Linux native suite passed 2,426 library tests with nine existing
ignores. The gateway framing and real service integration tests passed.
The suite used a private D-Bus session and Secret Service keyring. An
initial run without the keyring stalled in companion inventory tests.
Svelte checking reported zero errors and warnings.

The locked Windows suite passed 2,493 library tests with 18 existing
ignores. The gateway framing and real service integration tests passed
on the same Windows sources.

Run `node --test tests/ui/profile-samples-native.mjs` with the appropriate
browser or WebView environment from the
[local suite checkpoint](2026-11-09-local-suite-checkpoint.md#full-connected-path-on-browser-dev).
The fixture uses internal debug controls and processing nodes in a private
database. Physical input and simulator timing remain separate checks.

| Local artifact | SHA-256 |
| --- | --- |
| Linux desktop | `70e049cf7920806118e28cdf8e80ab6f1c3418ee5076f2437d001b5d17bb50e6` |
| Linux browser-dev gateway | `1d81bba051a6c10f43c984e0dea018d954ffdb34bf9723cff191760cf7f3732d` |
| Windows desktop | `72f653981b1779b9802752f50ba0d40b97ea01a89714ad0f5334974532f5cdc2` |
| Windows browser-dev gateway | `2ea36457d2b92ff1f0e3e06cffcf69efe0e4dbd1b9d29abf76e2d4ef681e925f` |
| WebKitGTK `1794271730167/evidence.json` | `628d45488af9da4434894ebb9134139bd1d6a9dc8f0168319983c2d655c0d83b` |
| Linux browser-dev gateway `1794271742382/evidence.json` | `13f1a39b620e32bc38af5c0a80cb405d83698e8c171d3c17a1cd51ccff0e102b` |
| WebView2 `1794272240017/evidence.json` | `eb82aedf5ee3e9e689d557b2c5827f157cc5fcb4aabfcff31efe0f9fbc0477f9` |
| Windows browser-dev gateway `1794272268995/evidence.json` | `274a234e581248dfecd943903409fc2fe8848bb86d86524ba8692528fea092fd` |
