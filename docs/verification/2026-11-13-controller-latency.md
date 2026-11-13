# Controller latency at the rig on 13 November 2026

The operator performed a deliberate Gladiator axis sweep and main-trigger
taps in the running Windows dev Cockpit. The private qualification executable
was built for `c3ee4370`. Its SHA-256 is
`49df4141ed3bc45ff98478fa971db5c69a1779621eff93e55ae81f4099df47c0`.
The frontend reused the previously verified isolated source and protocol
overlay. Each session used a fresh private database. No simulator was running.

## Profiles inspector

The focused, visible inspector observed 1,125 continuing Raw Input reports
over 15 seconds and matched 499 unambiguous visible axis transitions.
The native stream delivered eight coalesced preview reports. The diagnostic
reported no continuity, focus or clock errors. Clock uncertainty was
±0.601 ms and offset drift was -0.007 ms.

| Interval | p50 | p95 | p99 |
| --- | --- | --- | --- |
| Native read completion to journal | 0.279 ms | 0.351 ms | 0.395 ms |
| Native read completion to DOM | 8.225 ms | 10.679 ms | 15.308 ms |
| Native read completion to next frame opportunity | 15.498 ms | 17.269 ms | 22.373 ms |

The X axis reached native 0–4095 and presentation 0–32767. The Y axis
covered native 2027–4095 in this trace. The remaining axes stayed still.
Full endpoint qualification for every axis therefore remains open.
The first movement cue did not create its start marker and captured no
measurement. The successful trace comes from the explicitly repeated sweep.

## Physical trigger to vJoy

The operator identified the main trigger by a single hold and release.
It resolved to `hid:0009:0001:1:0:8`. A neutral preflight passed before
the measurement. The temporary profile routed this exact button to slot 1.
All 40 ordered press/release edges matched the independent Windows
Raw Input receiver. The receiver recorded 472 reports. Clock uncertainty
was 0.0023 ms. Stop freed the slot and the temporary profile was deleted
without cleanup errors.

| Native read completion to vJoy Raw Input receipt | p50 | p95 | p99 |
| --- | --- | --- | --- |
| Lower bound | 0.8645 ms | 0.9620 ms | 1.0111 ms |
| Upper bound | 0.8668 ms | 0.9643 ms | 1.0134 ms |

The diagnostic passed its destination latency budget. These intervals begin
at physical Raw Input read completion. Earlier switch/HID/OS delivery,
display scanout, axis destination timing, unplug/reconnect and simulator
effects remain separate acceptance gates in #199.

## Evidence and checks

All ten focused clock, axis-range and physical-vJoy diagnostic tests passed
locally before committing this record. No production code changed.

| Local trace | SHA-256 |
| --- | --- |
| `.research/reactive/profile-preview-latency/1794604830259.json` | `7690d9f5986fd0679cd060e2768efe92c6e53bdaa49d76de51f54e0cb3ac8523` |
| `.research/reactive/physical-vjoy-latency/2907f7cb-4489-4ba8-b0cb-b9ac1d3da8c5/evidence.json` | `1b31e860b4bf27654878f6d985e4e1dce7b37c234a10438247b097abeca121c7` |
