# Concurrent profile path and inspector readers

A 32-node path and a 16-node inspector with no shared topics exceeded the
previous browser context and native delivery worker limits. The expanded
fixture passed in the Linux WebKitGTK desktop and in Edge against the rebuilt
browser-dev gateway. It switched to a second disjoint 32-node path and back,
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

Run `node --test tests/ui/profile-samples-native.mjs` with the appropriate
browser or WebKit environment from the
[local suite checkpoint](2026-11-09-local-suite-checkpoint.md#full-connected-path-on-browser-dev).
The fixture uses internal debug controls and processing nodes in a private
database. Physical input, simulator timing and the expanded Windows WebView2
run remain separate checks.

| Local artifact | SHA-256 |
| --- | --- |
| Linux desktop | `70e049cf7920806118e28cdf8e80ab6f1c3418ee5076f2437d001b5d17bb50e6` |
| Linux browser-dev gateway | `1d81bba051a6c10f43c984e0dea018d954ffdb34bf9723cff191760cf7f3732d` |
| WebKitGTK `1794271730167/evidence.json` | `628d45488af9da4434894ebb9134139bd1d6a9dc8f0168319983c2d655c0d83b` |
| Browser-dev `1794271742382/evidence.json` | `13f1a39b620e32bc38af5c0a80cb405d83698e8c171d3c17a1cd51ccff0e102b` |
