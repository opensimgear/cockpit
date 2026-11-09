# Windows DCS command process validation

An instrumented `4fa4370b` Windows dev Cockpit delivered all 48 authored commands while the 1 kHz and 125 Hz controller fixtures routed to vJoy. The simulator command worker recorded each native validation step. The following times are in milliseconds.

| Step | p50 | p95 | Max |
| --- | ---: | ---: | ---: |
| Resolve configured executable paths | 0.223 | 0.284 | 0.299 |
| Enumerate matching processes and inspect candidates | 11.823 | 13.726 | 15.888 |
| Complete exact process proof | 12.791 | 14.598 | 16.799 |
| Check catalogue file metadata | 0.169 | 0.209 | 0.218 |

Two command attempts also ran the periodic full catalogue proof, which took 73.12 and 77.66 ms after the file metadata checks. This accounts for the occasional long catalogue delay seen in the earlier trace. The proof still reloads the captured documents to detect content changes that preserve metadata.

The trace is `native-51392-native.err` under `.research/c193-reconnect/dcs-native-stage-timing`, SHA-256 `686c17705c53c782e5b724e9f2a7791b6b4aea1896932736607917ea1c9145be`. Its full workload evidence is `.research/dcs-physics-native/1794224663567/evidence.json`, SHA-256 `9c587ba83ff81596ee32e65009ad0264785c9b27ba8cba11679ea3efac88cd45`. The diagnostic executable SHA-256 is `6ed59ea9ca40f21799750f5aed18b0a7e9cf000a54130b30a8a18e08a7ed46e9`.

On the same Windows host, an independent API probe alternated 100 samples per method. Tool Help took 4.54 ms at p95. The newer `SystemBasicProcessInformation` took 0.062 ms at p95. Both final samples listed 411 processes. Those timings measure API enumeration only. [Windows API documentation](https://learn.microsoft.com/windows/win32/api/winternl/nf-winternl-ntquerysysteminformation#systembasicprocessinformation)

Cockpit uses the newer list when available and falls back to Tool Help when it cannot obtain a complete list. Both paths use the same executable matching and validate full image path, creation time, and current liveness. The exact process proof still rejects ambiguous or inaccessible matching processes. Each command still requires a current process proof.

The isolated Windows candidate passed the full native library suite with 2,497 passed and 18 existing ignores. The parity test matched the current test process by PID, parent PID, and executable name against Tool Help. The configured production line coverage for `games/discovery/windows.rs` was 73.21%. LLVM reported 74.44% raw lines and 79.31% functions. The instrumented test build had no branch data.

## Running dev Cockpit comparison

Each run sent 24 debug commands and 24 browser-panel commands to the private DCS-BIOS UDP receiver under the same dual-rate vJoy workload. All times are milliseconds from submission or panel activation to the UDP callback. The panel values include the conservative browser clock bracket.

| Windows dev build | Debug p50 / p95 / p99 | Panel p50 / p95 / p99 |
| --- | ---: | ---: |
| Tool Help run 1 | 21.09 / 28.82 / 100.04 | 17.40 / 22.15 / 91.11 |
| Tool Help run 2 | 22.20 / 28.56 / 105.09 | 18.45 / 23.27 / 90.95 |
| New process list run 1 | 11.06 / 18.78 / 92.07 | 6.27 / 10.96 / 83.45 |
| New process list run 2 | 12.13 / 18.93 / 22.83 | 7.15 / 9.16 / 85.36 |

Both candidate runs delivered all 48 commands in order. Stop and reapply produced no replay, released the vJoy slot, and completed without cleanup errors. Both runs also verified that a competing DCS process withdraws telemetry and that the sole process can recover. The browser-panel p99 still reached 83–85 ms. Full catalogue proof latency, raw capture attribution and actual simulator effects remain open.

The candidate SHA-256 is `4ba227f28d7fbdbf293eed7a4c7ebc2ad71916d936ba0144aa2fe56b56c24c0a`. The run evidence is in `.research/dcs-physics-native/1794225065839/evidence.json` and `.research/dcs-physics-native/1794225127738/evidence.json`, SHA-256 `e0812dd1250fa5a635d65d1a686f304341aafe577540c0e6534e2e3e5fcb82b3` and `c4c3333c623e438ee0daf0c2c38dace574fc153914077df8d0f4e66ab3c9018c`, respectively.

## Catalogue content proof

The two-second proof now checks SHA-256 fingerprints of the bounded bytes parsed at capture. When every document matches, Cockpit reuses the captured interpretation. On a byte mismatch, it runs the existing parsed catalogue validation and compares the canonical revision. The metadata fence covers all captured paths, including absent optional documents, before and after content verification. The watch still confirms the baseline after registration, and command dispatch still checks metadata immediately.

The five catalogue documents used by the live fixture total 2,112,481 bytes. In a separate 20-sample Windows dev microbenchmark that read and hashed the same documents, the unoptimized Rust SHA-256 implementation took 44.19 ms at p95. The implementation from the locked `ring` dependency took 1.59 ms at p95. This measurement covers file reads and hashing. The application also checks metadata and performs its usual authority checks.

The Windows native suite passed 2,498 tests with 18 existing ignores. Ubuntu passed 249 focused DCS tests with three existing ignores. Both builds passed the metadata-preserving semantic edit, equivalent JSON rewrite, and watcher baseline regressions. Production line coverage was 95.75% for `dcs_bios.rs`, 95.22% for `dcs_profile.rs`, 98.08% for `dcs_passive_watch.rs`, and 94.55% for `catalogue_files.rs`. LLVM function coverage was above 84% in all four modules. The instrumented build had no branch data.

The Windows dev comparison reused the 48-command dual-rate workload and the receipt clock brackets above. The immediately preceding process-list build was the baseline for this change. Times are milliseconds from debug submission or panel activation to the UDP callback.

| Windows dev build | Debug p50 / p95 / p99 | Panel p50 / p95 / p99 |
| --- | ---: | ---: |
| Process list and parsed catalogue proof, run 1 | 11.06 / 18.78 / 92.07 | 6.27 / 10.96 / 83.45 |
| Process list and parsed catalogue proof, run 2 | 12.13 / 18.93 / 22.83 | 7.15 / 9.16 / 85.36 |
| Raw catalogue content proof, run 1 | 10.67 / 15.17 / 16.85 | 7.04 / 9.35 / 9.47 |
| Raw catalogue content proof, run 2 | 10.14 / 15.36 / 15.45 | 6.69 / 8.95 / 9.08 |

The candidate delivered all 96 commands in order. The runs verified no replay after Stop and reapply, a free vJoy slot, no fixture budget misses and zero cleanup errors. Capture-to-reducer attribution for these command effects, actual DCS effects and physical controller timing remain open.

The candidate SHA-256 is `d452dd8ef5126feb889a37fe9c4725549b45cd1dc01f13ae30b7932c9d6726c3`. The run evidence is `.research/dcs-physics-native/1794225804632/evidence.json` and `.research/dcs-physics-native/1794225880308/evidence.json`, SHA-256 `f58a1b448e344c9b6933b78e03c6982bfac95d92e104877a4853aede92ab3e21` and `71447142067e696da71f69610ff81777d96258920ec1807470a84efd140a55f1`, respectively.

## Matched command capture and reducer timing

An isolated diagnostic Windows dev build of `cc68085b` recorded the direct-source observations of the two authored DCS actions. Each observation had to match the capture timestamp, source port and sequence in the same reducer batch. The trace recorded the generation of the matched frame. The analysis then paired the ordered action sequences and command texts with the private UDP receipts. Every command used one validation attempt.

The capture mark is where Cockpit stamped the authored debug input or captured the browser-panel update on the host. The reducer mark bounds completion of the accepted batch. The transport mark is immediately before the socket send, and the final mark is the receiver's callback on the Windows monotonic clock. The input offer and browser clock bracket are recorded separately. Each table entry is p50 / p95 / p99 in milliseconds.

| Trial and source | Capture cadence | Capture to reducer batch end | Effect drain to transport | Transport to UDP callback | Capture to UDP callback |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1, debug controller | 180.24 / 187.04 / 188.22 | 1.74 / 2.28 / 2.45 | 5.11 / 6.09 / 6.44 | 0.07 / 0.10 / 0.24 | 6.93 / 8.47 / 8.77 |
| 1, browser panel | 180.10 / 184.35 / 186.55 | 1.49 / 3.29 / 3.35 | 3.63 / 4.56 / 5.05 | 0.06 / 0.07 / 0.10 | 5.44 / 7.24 / 7.90 |
| 2, debug controller | 180.15 / 183.74 / 185.82 | 1.76 / 2.28 / 2.29 | 5.21 / 6.12 / 6.48 | 0.07 / 0.23 / 3.17 | 7.16 / 8.90 / 10.20 |
| 2, browser panel | 180.28 / 184.29 / 186.37 | 1.54 / 4.10 / 4.34 | 3.91 / 5.32 / 5.52 | 0.07 / 0.08 / 0.08 | 5.66 / 8.57 / 8.66 |

All 96 receipts matched. In each trial, the independent vJoy load offered 4,801 and 601 frames at measured rates of about 1,000 and 125 Hz over 4.8 seconds of overlap. Peak pending frames were 17 and 15 in the first trial and 17 and 11 in the second. The command fixture waited for each receipt before issuing the next command. Both runs recorded zero fixture budget misses, no replay, a free vJoy slot and no cleanup errors. Command lease margins, physical controller acquisition, display presentation and simulator effects require their separate evidence.

The diagnostic executable SHA-256 is `d0b75a798d158ef4c9e769fe5b2b08306d42c35cc30a5ded6104b15dbde57405`. Under `.research/c193-reconnect/dcs-native-stage-timing`, the paired reports `capture-stages-1.json` and `capture-stages-2.json` have SHA-256 `52bac746217678c199623abc9b464f61526bcd73e66d260126f0a4abbdd03bec` and `a1535604b4ea7702825dc046cc947ac84406ec885164834c1d0b3c3c2c0d4c44`. They record per-edge stages and the hashes of the underlying native logs and complete workload evidence.

## Command deadline and telemetry lease margins

The same diagnostic fixture recorded three existing deadline margins from the native admission callback immediately before socket submission. They are the remaining 100 ms command deadline, the decoded owner's freshness deadline, and the freshness deadline of the reader's last delivered snapshot. All three use the same monotonic clock. The analyzer pairs each measurement with the original source observation, action sequence and UDP receipt.

| Trial and source | Commands | Minimum command deadline remaining | Minimum decoder freshness remaining | Minimum reader freshness remaining |
| --- | ---: | ---: | ---: | ---: |
| 1, debug controller | 24 | 97.967 ms | 560.149 ms | 553.490 ms |
| 1, browser panel | 24 | 95.761 ms | 558.026 ms | 558.026 ms |
| 2, debug controller | 24 | 97.985 ms | 559.746 ms | 559.746 ms |
| 2, browser panel | 24 | 93.596 ms | 557.292 ms | 557.292 ms |

Both live Windows dev trials retained the dual-rate vJoy workload and matched all 96 commands in order. The analyzer required positive margins for each individual command, a free vJoy slot, zero fixture budget misses and no cleanup errors. Debug capture to UDP callback p95 was 8.30–8.42 ms. Panel capture to UDP callback p95 was 8.43–10.72 ms. Actual simulator effects and physical input and display timing remain in the separate acceptance follow-up.

The diagnostic executable SHA-256 is `55485a40fb4cf0857daea6d3842cf2d84d60d8e825605441ecf93017558e023a`. The paired reports under `.research/c193-reconnect/dcs-native-stage-timing` are `command-lease-stages-1.json` and `command-lease-stages-2.json`, SHA-256 `9c59af5fc6c2822166ff9503b2f0baa9cb827919c037bd5216283173a2e781b4` and `26c5c64304494840a1f7f1fa76430ea1f5b75bea35846779c470055226910624`. They contain the per-command margins and hashes of the native traces and workload evidence.

## Concurrent command producers under dual-rate vJoy load

The private Windows fixture offered a debug press and a compiled browser
panel press concurrently, then verified exactly one private UDP receipt
from each source before releasing both controls. It repeated that pair
24 times at a nominal 180 ms interval. Each run also offered 4,801 and
601 ordered transitions at 1 kHz and 125 Hz to vJoy. The window with
both load lanes active lasted 4.8 seconds.

| Trial | Paired offer skew upper p95 / max | Debug offer to UDP p50 / p95 / p99 | Panel activation to UDP upper p50 / p95 / p99 |
| --- | ---: | ---: | ---: |
| Windows dev 1 | 5.04 / 7.05 ms | 13.88 / 23.96 / 28.19 ms | 7.89 / 11.58 / 12.05 ms |
| Windows dev 2 | 7.59 / 8.66 ms | 10.35 / 25.90 / 29.98 ms | 8.73 / 11.58 / 16.14 ms |

Across both runs, the receiver matched all 96 commands and every vJoy edge,
including both Stop releases in each trial. Stop and reapply produced no command replay
and returned the slot to free. The highest pending depth was 19 frames on
the fast lane and 18 on the peer. The fast MCP reader recorded 630 and
640 updates while another reader stayed unread for at least 12 seconds.
Cleanup reported no errors.

The offer timings start before the command reaches Cockpit's capture
boundary. The panel timings include the upper end of a 0.41–0.43 ms
browser clock bracket. The debug offer-to-callback p95 was 24–26 ms.
The R9 native latency target starts at the host capture stamp.

The first attempt stopped before routing because the private seed named
a catalogue path absent from the clean frontend export. The reruns used
the existing authored catalogue as the read-only copy source. The
concurrent command fixture reached 96.64% lines, 91.94% branches and
97.56% functions in both running dev trials.

The dev executable from `d996b19e` has SHA-256
`0a722a56cb4b1726d516245bbb7f925114c2c37e455fd7c6fcb4df0f8a3d1c47`.
The frontend is from `88ee864e`. The private concurrent command fixture
has SHA-256
`ed544c70a5fe07e142c885673da6ccd5b354f25d00eaab1cce86002368efb1d6`.
The evidence files are under the clean frontend's `.research/dcs-physics-native/`.

| Trial directory | `evidence.json` SHA-256 |
| --- | --- |
| `1794235857729` | `8069294ff784cc37301877d1cfc50c44a429959e4f57b356ff03ba0c12fb3754` |
| `1794235934149` | `2e46fb8ec05a3d5fae2085b757e938ee902791265cb24b24ee664954f1d1f590` |

### Native capture timing with concurrent producers

The earlier `cc68085b` diagnostic executable paired each source capture
and reducer batch with the ordered UDP receipts and the lease margins.
The command worker, DCS sender and delivery validation source files match
the current dev revision. The builds differ in other Cockpit code and in
the diagnostic probes.

| Trace sink and source | Worker queue p95 / max | Host capture to UDP p50 / p95 / p99 |
| --- | ---: | ---: |
| File, debug | 11.881 / 12.218 ms | 8.068 / 22.466 / 23.056 ms |
| File, panel | 13.443 / 38.090 ms | 17.343 / 23.875 / 43.809 ms |
| Memory run 1, debug | 0.023 / 0.091 ms | 7.916 / 11.037 / 11.859 ms |
| Memory run 1, panel | 0.019 / 0.019 ms | 7.149 / 11.936 / 13.229 ms |
| Memory run 2, debug | 0.021 / 0.085 ms | 7.612 / 10.721 / 11.201 ms |
| Memory run 2, panel | 0.019 / 0.023 ms | 6.915 / 11.065 / 11.490 ms |

In the file-backed trace, the slowest panel command spent 38.09 ms between
queueing and worker entry. The preceding command's sender had already
returned 2.59 ms before the panel command was queued. Context validation,
catalogue validation and delivery-token validation then took 1.04 ms,
0.16 ms and 0.16 ms respectively. With stderr drained into memory and
written after exit, the tail did not recur in two trials. The trace
setup and host scheduling are possible contributors to the difference.

Both memory-drained trials met the native 20 ms p95 and 50 ms p99
targets. Capture-to-reducer batch-end p95 was 2.81–3.08 ms for debug
and 7.60–7.81 ms for the panel. Before capture, debug offer-to-capture
p95 was 14.66–15.23 ms, compared with 0.92–0.95 ms for panel
activation-to-capture. Every command used one validation attempt.
At socket submission, the smallest remaining command deadline was
98.03 ms. Decoder and reader freshness had at least 558.75 and
554.67 ms remaining respectively.

All three diagnostic trials matched all commands to their source sequence,
generation and receipt. The executable hash is in the lease section
above. The analysis files under `.research/c193-reconnect/dcs-native-stage-timing/`
also record the raw trace and workload hashes. Physical input, actual
simulator effects and the wider R9 matrix remain open.

| Analysis file | SHA-256 |
| --- | --- |
| `command-pairs-diagnostic-stages.json` | `e73e2cbb9c1aae8b352e409d3eeffc3fb13a1ed1ee0b94337739925b4e7dd372` |
| `command-pairs-memory-stages.json` | `46917e9d1db411fcb94f44676520cd5afda590d10af358d6059ab206a10ee2e4` |
| `command-pairs-memory-stages-2.json` | `899addaa9714996003cc5e0feadbf277818407b8f4bb70cd72c8e0cccd06b992` |
