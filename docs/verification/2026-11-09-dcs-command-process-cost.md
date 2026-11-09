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
