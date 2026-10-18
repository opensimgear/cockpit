# Native DCS helicopter exports — 18 October 2026

The running Windows Cockpit and real DCS missions exercised the installed MH-6J, AH-6J and Cayuse/OH-6A export hooks. The user operated DCS; commands were applied through saved profiles in the native Cockpit. Actual returned simulator channels were inspected separately from requested action values. Passive UDP captures supplement the native inspection; no telemetry was injected.

## Generated catalogues and live observations

| Aircraft | Generated aircraft catalogue | Live result |
| --- | --- | --- |
| MH-6J | Empty aircraft module, with CommonData and NS430 aliases | Matching aircraft identity, advancing export counters and CommonData altitude 524 reached the running native profile and its identity route. No aircraft-specific commands are advertised. |
| AH-6J | 209 entries: 127 clickable controls and 82 gauges; 160 outputs | Native commands changed returned `MPS_POSLIGHTDIM` feedback from 4 to 0 and back to 4. Altitude 524 and raw ADI bank samples remained fresh. |
| OH-6A / Cayuse | 152 entries: 61 clickable controls and 91 gauges; 146 outputs | Native commands changed returned `LIGHTPANELS_POS_LIGHT` feedback from OFF (1) to BRT (2), then back to OFF (1). The user visually confirmed both BRT and the final OFF position. Altitude 96 and raw ADI bank samples remained fresh. |

Both aircraft catalogues inherit CommonData and NS430. Gauge values represent normalized cockpit arguments encoded as integers, rather than calibrated physical quantities. The AH-6J test confirms command dispatch and returned argument changes; its visual switch-position check was inconclusive and is not claimed as passed. This is a representative live control test, not an exhaustive in-game test of every exported control or gauge.

The final Cayuse OFF capture contained 105 distinct completed-export counters in four seconds and returned position 1. The validation profile was stopped after the check. The clearly named local validation profiles remain saved for review. Cockpit dev remains running.

## Runtime fixes exercised by the live run

DCS sends multicast exports from the selected local network interface while accepting commands on loopback. The transport now accepts that exact, explicitly configured interface when multicast is enabled and the command host is loopback. It retains exact command-host acceptance and rejects unrelated senders; commands continue to use the configured destination. The local integration was configured with an explicit multicast interface.

A lengthy profile activation could leave more than 64 queued DCS datagrams before command admission. Activation now discards that backlog by reopening its receive socket, clears decoder/freshness witnesses, and waits up to three seconds for new aircraft identity and advancing counters before admission. Normal running overflow still disarms the branch. Baseline failures now include their underlying diagnostics.

The pre-arm wait belongs to `NativeProfileHost` at the initial Baseline phase and both Start checks. Each bounded poll checks the native admission epoch before and after reading and keeps an already acquired neutral virtual output leased. The three-second deadline is unchanged; `GameSources` performs a single bounded read. A hardware-free regression uses the actual Session/Feeder with an in-memory driver: 80 queued old held frames are discarded, a 350 ms wait exceeds the unchanged 250 ms output lease, and two fresh neutral export counters produce the baseline without arming the reducer or submitting a report. Concurrent epoch cancellation returns in under two seconds. Running polling retains its overflow-disarm policy.

The ordinary running Cockpit dev UI then applied saved **DCS hook live validation**, revision 3 (`1adb093a-a0ae-473e-b334-faad7f090335`), once in the real MH-6J DCS context. It reached Running in session `d1ec6fe5-956e-4976-9063-92b51c3d353f`, generation 1. Exact-owner inspection showed `dcs/port-1` and Identity input/output at 524 in two fresh samples: observed/captured timestamps 153300578/153321669 us (21.091 ms age), then 168300808/168324224 us (23.416 ms). The native sample timestamps advanced. Commands remained configured for loopback, with the explicitly selected local export interface.

This verifies the telemetry-to-Identity route. Retained summary: `.research/dcs-telemetry-dev-20261018/ui-telemetry-summary.json`, SHA256 `f9803faaac82c75be770024ae7d876018ed9909373483f4c1c228f8d0da4e441`; raw UI observations SHA256 `c26718c45bf450074bb73ebe048f8cecd947592143382dc78ba674124d2e7549`.

## Validation and retained evidence

- `dcs_bios::tests`: 30 passed, including exact-interface sender acceptance, foreign-source rejection and unchanged command destination.
- `profile_session_games::tests`: 12 passed, including activation backlog discard, fresh identity/counter reacquisition, stale-data protection and running overflow disarm.
- `profile_session_host::`: 52 passed, 2 existing ignores; includes the 12 launch-host tests and the real-clock output-lease/cancellation regression.
- `git diff --check`: passed.

Local evidence is retained under `.research/dcs-aircraft-exports/live/`: native MH-6J telemetry, AH-6J position 0/4 inspection, OH-6A position 2/1 inspection, passive wire captures and regression logs. The earlier AH-6J wire capture named `ah6-position-0-wire.json` captured the pre-command state 4; the native position-0 inspection is the evidence for state 0. The final OH-6A position-1 capture was taken after Apply completed.

The follow-up copied native artifact passed all 94 focused tests (30 DCS, 12 GameSources and 52 Host), with all 390 recorded inputs unchanged. Its source/artifact/result manifest is `.research/p9-dcs-interface-source/final-2/final-manifest.json`, SHA256 `2e0c0460bf162ad3c43f1a3b3485af09784379ef10356a3973eaf81ff94e6048`. These tests use no local GatewayManager, physical driver or game process.

The previous setup checks verified detected-mod-only installation for all, absent and partial mod sets. This live run validates the resulting installed hooks. Upstream documentation asset generation reported missing `doc_assets` paths; JSON catalogues were generated and the tested telemetry/control paths worked. No OSG hardware, combined joystick, HidHide or browser-companion acceptance is implied by this helicopter-hook run.
