# DCS-BIOS receive ownership — 2026-11-02

This is another acquisition step for #192/R3 and #188. It does not complete the
decoded telemetry, read-model or routing migration.

## Changes

DCS-BIOS previously read its shared UDP socket only when a profile or inspector
asked for data. One cancellable native receiver now acquires accepted datagrams
independently and commits their original `Instant`, producer identity and checked
sequence to bounded reader queues. Passive reads dequeue committed data and never
call the operating system receive API. A blocking readiness API supports future
decoded producers without introducing an idle refresh timer.

Datagrams share one immutable byte allocation across readers. There are at most
64 readers, 256 pending datagrams and 1 MiB of logical pending bytes per reader.
Reader overflow clears its partial history and permanently revokes that reader;
healthy readers continue independently. Native receive/counter failure withdraws
all affected readers and releases the socket. New acquisition retires a terminal
owner before rebinding, while retained old handles remain failed. Initial reader
registration precedes receive startup.

Freshness settings and command ports do not duplicate acquisition. The source
filter and multicast/binding policy remain authoritative: incompatible live
policies on overlapping fixed-port binds are rejected instead of stealing unicast
packets through `SO_REUSEADDR`. Distinct concrete bind addresses remain supported.

Subscriber continuity is checked under the receive state lock through nonblocking
command dispatch. A just-overflowed or failed reader cannot send using its cached
decoder freshness before its next poll. Command syntax, destination, higher-level
owner/catalogue fences and no-retry behavior remain enforced. Socket submission
still does not prove a simulator effect.

Profile/tracker decoders correlate native capture age into their existing source
clock once per drain. Already-expired queued exports, invalid clock correlation
and producer/capture discontinuity cannot revive stale values. Existing BIOS
counter and aircraft-baseline guards remain independent. Existing mapping frame
time remains ordered admission time: backdating it to an older BIOS sample would
violate the reducer's lifecycle-event ordering, especially with mixed ownship
updates. Native capture time and decoder age remain separate evidence. R4 still
needs to carry source capture and admission clocks explicitly through routing.
Correlation uses the existing millisecond decoder resolution; it does not claim
upstream sender timing or sub-millisecond destination latency.

## Remaining boundaries

Decoded catalogue-dependent state is still reduced by the existing consumers.
Decoder expiry ticks, tracker observation, process/catalogue validation and UI
refreshes still need their remaining R3–R7 cutovers. This commit does not rename
those consumers as event-driven producers. Other integrations, MCP observation,
high-rate and actual simulator/hardware acceptance remain open. #199 remains first
in progress.

## Verification

- Complete isolated native regression: 1,685 pass, zero fail, 12 existing opt-in
  checks ignored. After the final restoration of existing admission timestamps,
  all 139 affected instrumented BIOS/decoder/tracker/profile tests pass (one
  separate existing opt-in decoder check ignored), plus the running fixture below.
- Production line coverage: transport 97.90%, capture correlation 100%, BIOS root
  95.94%, profile decoder 95.09%, tracker polling 95%, tracker sources 84.40%,
  logical profile-session module including read/prepare/delivery fragments 91.18%.
  All changed-module coverage floors pass. Coverage harness: 20 tests pass.
- Native executable builds. The actual Windows cockpit fixture passes all
  inspection/profile/setup/repair/pause/permission/frozen-time/stale/respawn/stop/
  process-exit phases in 53.93 seconds. BIOS gauges remain live while ownship
  expires. Cleanup reports zero errors, restores original installations/settings,
  and adds no scan root. Completion: `2026-11-02T17:00:00.538Z`; local evidence:
  `.research/dcs-physics-native/1793638746614/evidence.json`.
- Tested executable SHA-256:
  `b436bf67afd0cdc60327f0ccf1855998e2d1082e812c093bc9293a4fc3bee736`.
  Isolated build tree: `4e8bc67d0eb223d67c3de2359f4a673b7e4a42d8`.

The native fixture authors its simulator process and telemetry and uses no
physical output. It does not qualify actual DCS, raw-input-to-destination latency,
upstream network loss or a complete-app idle/soak workload. Raw local artifacts
remain in ignored `.research/reactive/r3-bios-*` paths. The preceding ownship
commit `d3cc174` passed GitHub CI.
