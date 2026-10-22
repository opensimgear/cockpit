# Board baseline capture for downloadable rendering

Status: instrumentation, native verification and initial stopped-input physical redraw captures are complete on both boards. Live USB/WSS sessions and the full qualification soak remain pending. This is the measurement gate for milestone 0 of [the implementation plan](../../companion-render-packages.md). Existing Sunton USB/touch qualification does not establish the new renderer's timing or Wi-Fi/TLS memory headroom.

## Measurements available in engine firmware

The fixed `core/diagnostics` store uses at most 2304 bytes of internal RAM, enforced at compilation. Hot paths perform bounded counter updates only; there is no diagnostic allocation, formatting, logging or filesystem access while rendering or polling. ESP-IDF updates/snapshots use a short critical section. Instrumentation overhead is included where a measured operation contains nested instrumented calls, so hardware measurements must identify this instrumented build.

| Metric | Scope and units |
| --- | --- |
| `lcd-spi`, `touch-spi` | Sunton synchronous SPI transaction microseconds, attempted bytes and failures; display commands count too |
| `lcd-write` | Sunton pixel-span call including wait, setup, conversion and transfer; bytes are requested pixel bytes, not confirmed bytes after failure |
| `lcd-bus-wait`, `touch-bus-wait` | Sunton mutex acquisition duration/failures, retaining the existing 100 ms timeout |
| `touch-poll` | Complete board touch callback on either board, including bus wait; failures preserve input cancellation |
| `touch-poll-interval` | Start-to-start poll interval, including scheduling and previous callback time |
| `render` | Raster composition and synchronous pixel writes; excludes `present` |
| `present` | Optional board presentation/fence callback; Sunton reports zero samples |
| `host-request` | Complete socket transaction, including serialization/transport/response handling; failures count too |
| `source-ack`, `input-source-ack` | Request-start to authenticated source-ACK validation, with millisecond resolution; input subset excludes baseline/heartbeat. Neither implies controller submission or simulator confirmation |

Each metric reports count, failures, dropped samples, total microseconds, exact maximum and p50/p95/p99 **upper bounds** from powers-of-two buckets. The last bucket's upper bound is `UINT64_MAX`. Percentiles include failed samples. Counts saturate rather than wrap; dropped samples invalidate a qualification run. Zero samples mean the path was not observed. Raw touch coordinates, control values, panel identifiers, addresses and credentials are not collected.

Internal 8-bit-capable heap free/minimum/largest-block values and task stack high-water marks are sampled once per second. Stack slots are application main, touch and network tasks, in bytes as reported by ESP-IDF. Zero sample counts identify missing task/path observations. Heap minimum and stack high-water marks are historical since boot/task creation; diagnostic reset does not reset SDK minima. The minimum sampled largest free block cannot prove every transient allocation.

While input prepares or arms, maximum queue depth and pending-input/touch/source-ACK/active-render ages are sampled **before** monitoring/polling can clear a queue or refresh a timestamp. Limits remain 250 ms for pending input, 600 ms for touch/render and 1000 ms for authenticated source ACKs. Capture fault/status evidence too: a low percentile cannot prove no unintended disarm occurred.

`serialRequestTxBytes` counts successfully written encoded request envelopes plus LF; `serialRequestRxBytes` counts bytes read during transactions, including envelopes/interleaved records. They exclude boot logs, standalone status/diagnostic output and serial-open/close messages. WSS counters measure transport application payload bytes, including received control payloads; they exclude TLS/IP/WebSocket framing and outgoing pongs. Use a host trace for total physical traffic and message classifications. These counters do not replace the variable-only PoC trace.

## Capture boundary

On the existing trusted serial command channel, start a window while input is stopped:

```json
{"command":"stop"}
{"command":"diagnostics","reset":true}
```

Reconnect the test session, wait for baseline/display readiness and execute the workload. Stop before snapshotting:

```json
{"command":"stop"}
{"command":"diagnostics","requestId":"sunton-usb-baseline-1"}
```

Diagnostics reject armed/preparing input; they never silently disarm. Explicit `stop` performs source cleanup before slow UART output. Output is one correlated `diagnostics-summary`, twelve `diagnostics-metric` records and `diagnostics-complete`. There is no unsolicited diagnostic stream during input. Uptime and last diagnostic-reset uptime define the window; counters begin at boot if never reset.

Keep one serial connection open when opening the USB bridge resets the board. Reset loses volatile counters. For direct USB, Cockpit owns the port: send/capture diagnostics through its already-open connection. A second process must not compete, and reopening is not evidence of a continuous soak. A host command/capture action on that link is still required to automate direct-USB qualification. Firmware command and parser provide its boundaries.

For an unowned port, Python 3 plus pyserial can stop and snapshot:

```sh
python apps/companion-esp32/tools/baseline.py --port COM6 --baud 115200 \
  --minimum-window-seconds 1800 \
  --firmware apps/companion-esp32/build-sunton-3248s035r/opensimgear_companion_esp32.bin \
  --output .research/companion-baseline/sunton-usb.json
```

Use 921600 for Waveshare. The helper does not flash, provision, reconnect or arm. It rejects shorter windows, including a reset on open. It cannot verify the supplied firmware file is installed; JSON records this limitation. `--reset` instead of `--output` stops input and clears counters. Reopening afterward may reset them again. A persistent serial monitor can control/capture Wi-Fi tests while WSS carries the host session.

Parse an existing host/monitor log without copying unrelated or sensitive records:

```sh
python apps/companion-esp32/tools/baseline.py --log .research/board-monitor.log \
  --request-id sunton-usb-baseline-1 --minimum-window-seconds 1800 \
  --output .research/companion-baseline/sunton-usb.json
```

The parser validates correlation, the complete metric set, counters and interval, writing only allowlisted fields. Operator-supplied logs are labelled and do not alone establish physical provenance. Keep raw setup logs out of Git.

## Baseline matrix and decision gate

For each board run direct USB and Wi-Fi/WSS separately, using unchanged legacy rendering first. Record board revision, flash/PSRAM detection, pinned IDF, verified firmware/app hash, source/publication, transport/baud and window. Use deterministic host feedback, recording-mode inputs and screen photos. Cover startup, full redraw, alternating disjoint updates, numeric/gauge changes, repeated large-area damage, hold/release, drag out, reconnect and a 30-minute feedback/input soak. Separate startup from steady percentiles.

Sunton retains its 64-byte non-DMA FIFO and 24 MHz LCD/2 MHz touch clocks. Measure transaction/setup cost, spans and contention before prototyping bounded transfer batching. Require before/after evidence, synchronous buffer ownership and bus slices meeting existing leases. Do not hold the bus through an unbounded full redraw or extend leases to conceal overruns.

| Board / transport | Heap minimum / sampled largest-block minimum | Main/touch/network stack minimum | Render/present p95 upper / max | Touch wait/interval max | Input ACK / queue age max | Unintended disarms |
| --- | --- | --- | --- | --- | --- | --- |
| Sunton USB 115200 | Pending physical run | Pending | Pending | Pending | Pending | Pending |
| Sunton WSS | Pending physical run | Pending | Pending | Pending | Pending | Pending |
| Waveshare USB 921600 | Pending physical run | Pending | Pending | Pending | Pending | Pending |
| Waveshare WSS | Pending physical run | Pending | Pending | Pending | Pending | Pending |

Results determine qualified profile memory, dirty-area/cadence and runtime byte budgets. Partition fit and native timing mocks cannot fill this table.

## Initial physical redraw captures, 16 October 2026

Both application images were installed using Cockpit's existing chip/security checks and verified application-only writes at `0x10000`; setup/NVS and partition tables were preserved. Waveshare used automatic boot entry; Sunton required manual BOOT/RESET entry. The source workspace subsequently moved to `C:\Users\ivang\_pws\opensimgear\cockpit`; these captures identify exact installed application bytes independently of source paths.

One serial connection stayed open per capture. After stopping input and resetting diagnostics, alternating embedded page selection forced complete disconnected-screen redraws approximately every two seconds. No host input or telemetry session was armed. No diagnostic samples were dropped and observed operations reported no failures.

| Observation | Sunton, COM6 / 115200 | Waveshare, COM5 / 921600 |
| --- | --- | --- |
| Window / redraws | 61.540 s / 29 | 62.552 s / 30 |
| Free / historical minimum internal heap | 123836 / 121840 B | 116275 / 114267 B |
| Minimum sampled largest free block | 110592 B | 73728 B |
| Main / touch / network historical stack minimum | 5444 / 7532 / 10056 B | 6248 / 6968 / 9836 B |
| Render mean / exact maximum | 288147 / 298754 us | 221706 / 229883 us |
| Render p95 upper bound | 524288 us | 262144 us |
| Present exact maximum / p95 upper bound | No separate present callback | 107814 / 131072 us |
| Touch poll exact maximum | 706 us | 512 us |
| Touch poll interval exact maximum | 21001 us | 20048 us |
| Touch bus wait exact maximum | 698 us | Not instrumented for this board |
| LCD SPI calls / attempted bytes | 179200 / 8700160 B | RGB framebuffer path; not SPI |

Evidence: [Sunton capture](evidence/sunton-3248s035r-stopped-redraw.json) and [Waveshare capture](evidence/waveshare-lcd7-s3-stopped-redraw.json). Application SHA-256 values are `d8c35011d40fc582884b1914839e82867d8f122e2ff4555a358226c66f249f20` and `f72a8bf13c821acc258d049c1e16268b9eac963b6450e901d8a714872731fd70`, respectively. Both builds used pinned ESP-IDF v5.5.2 and include diagnostics; this is not a comparison against an uninstrumented image.

These results establish initial redraw cost and idle heap/stack observations. They do not qualify TLS headroom, input ACKs, queue ages, touch accuracy, alternating damaged-region visual correctness or a 30-minute soak. Zero input/ACK samples mean unobserved. The full-screen measurements do not support a claim of full-screen 5 Hz on Sunton or 10 Hz on Waveshare. The proposed 5/10 Hz instrument-region caps remain candidates until bounded partial-update workloads pass. Keep the existing three-VSYNC ownership fence; measure dirty-region work before changing cadence or driver synchronization.

## Software verification

Run from `cockpit`:

```sh
python3 apps/companion-esp32/test/diagnostics.py
python3 apps/companion-esp32/test/sunton.py
python3 apps/companion-esp32/test/network.py
python3 -m unittest discover -s apps/companion-esp32/test -p baseline_test.py -v
```

The sanitizer/coverage harness checks histogram bounds, maxima, saturation, reset, invalid indices, memory/stack minima and lease maxima. Sunton mocks verify measured 100 ms lock timeouts, byte counts and SPI faults while retaining rotation/release checks. Client tests reject armed diagnostics and preserve lease cancellation. Parser tests reject incomplete/duplicate/corrupt snapshots and discard unrelated secrets. CI runs the checks and retains coverage. These are software results, not physical timing measurements.
