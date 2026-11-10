# Windows Assetto acquisition timing

The approved shared-memory reader aims for a 2 ms check while healthy
and a 50 ms retry after a read error. On this Windows host, the old
portable park waited about 15.6 ms at a time. The reader now uses one
private high-resolution timer and an owner cancellation event. The
change requires no process-wide timer resolution setting.

The Windows probe creates UUID-named private pages for each supported
layout. One writer uses its own high-resolution timer, and one
ordered subscriber drains on the production stream wake. Both use the
same monotonic clock. Each store has a before-and-after bracket and each
receipt records the original native physics capture timestamp and
sequence. Captures that may have started before the store completed
are counted separately from detection timing.

The table uses two uninstrumented debug builds. Detection percentiles
are the upper bound from the store bracket to capture. They describe
received packets, so delivery counts are shown separately.

| Fixture | Old / new actual writer Hz | Old / new packets received of 240 | Old detection p50 / p95 / p99 | New detection p50 / p95 / p99 | New capture-to-reader p95 | Old / new worker CPU as % of one core |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Assetto Corsa | 97.42 / 98.01 | 159 / 240 | 4.50 / 8.98 / 9.11 ms | 1.02 / 2.02 / 2.03 ms | 0.080 ms | 0.63 / 2.55 |
| Competizione | 189.63 / 192.18 | 82 / 240 | 2.70 / 5.05 / 13.61 ms | 1.05 / 2.04 / 2.07 ms | 0.103 ms | 1.23 / 6.26 |
| EVO | 188.44 / 191.88 | 82 / 240 | 2.09 / 4.62 / 9.19 ms | 1.05 / 1.81 / 2.10 ms | 0.130 ms | 0.00 / 5.00 |
| Rally | 61.60 / 61.87 | 239 / 240 | 7.24 / 14.39 / 15.12 ms | 1.02 / 2.02 / 2.03 ms | 0.069 ms | 0.00 / 2.42 |

The new reader preserved all 960 packet IDs and their original
acquisition sequences, with no duplicates, order regressions or
ambiguous store brackets. Each worker retired after its reader was
dropped. CPU was measured on the acquisition thread via Windows thread
accounting, which advanced in 15.625 ms increments on this host. A
zero on a short stage means the accounting counter did not advance.

The instrumented probe also delivered all 960 packets. The full
Windows native suite passed 2,509 tests with 19 explicitly ignored.
Line coverage was 88.12% for the shared source and 74.14% for the new
wait module. The cancellation regression verifies that a shutdown
interrupts a long wait and remains signalled for any subsequent wait.
The rebuilt dev Cockpit also discovered each of the four private
fixture processes, showed the scoped Allow action and withheld
telemetry while Windows access was denied.

Run the opt-in probe on Windows with
`cargo test --manifest-path src-tauri/Cargo.toml --lib private_pages_through_shared_worker_report_per_game_timing_and_delivery -- --ignored --nocapture --test-threads=1`.
The old-worker comparison used the same independent writer and the
original portable park. Local logs are under
`.research/c193-reconnect/reader-budget-max`.

| Log | SHA-256 |
| --- | --- |
| `windows-assetto-acquisition-independent-baseline.log` | `235cefcf3dd779709984a1b37f8e4636528af6d7e2891cf78024a61191b6ca60` |
| `windows-assetto-acquisition-hires.log` | `67083a552717293ad9be38aa6da28f01cb9eeaa9ea66905af92bc60550bbefbd` |

The probe covers the Windows acquisition worker and stream receipt.
The simulator's actual write cadence, scoped permission grant and
live telemetry paths remain in #199.
