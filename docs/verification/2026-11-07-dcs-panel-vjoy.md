# Combined DCS pause, browser panel and vJoy

One running desktop Cockpit profile routed authored DCS ownship telemetry
to a paired compiled browser panel and routed the panel press and an
independent debug source to separate buttons on the same vJoy slot.
The DCS observer held one acknowledgement while its independent reader
received 36 updates. The real browser worker delivered host lamp feedback
that went fresh, stale on ownship pause, and fresh again on resume. The
applied profile owner stayed the same.

Windows Raw Input received all 65 ordered debug transitions and the panel
press. Both buttons stayed held until Stop and released 1.91 ms after its
submission on the Windows performance counter. The slot became free and
the fixture restored the original private installation and provider state
with no cleanup errors.

Run `node --test tests/ui/dcs-physics-native.mjs` against a disposable native
Cockpit with a seeded private DCS installation and a valid DCS-BIOS copy.
Set `OSG_DCS_PHYSICS_NATIVE=1` and `OSG_DCS_COMBINED_VJOY_PROBE` to the
rebuilt Raw Input probe. The evidence is in
`.research/dcs-physics-native/1794038075714/evidence.json`. The dev build
from `3592e502` has SHA-256
`16781f2732f42e6d689ec9c740489c8d08a8bbb5cd13f5c78b5a3db575802ac5`.
The compiled publication is `scripts/fixtures/compiled-flight-publication.json`.

The separate older DCS-BIOS browser protocol fixture failed the current
compiled-panel asset negotiation. The combined run uses the compiled
publication and authored UDP. The 30-minute rig soak and real simulator
effects remain in #199.

## Routing with previews closed

The follow-up opened the Cockpit Debug telemetry inspector and observed its
native `games.telemetry` subscription and visible channel table. After the
stalled-reader phase it closed the two test readers and navigated from Debug
to Overview. The DevTools trace confirmed successful unsubscribe responses
for all three tokens. It recorded zero new preview subscriptions and zero
inspection reads during the closed phase.

The same applied owner sent a fresh host lamp update to the browser panel.
Windows Raw Input recorded both additional vJoy transitions after the readers
retired. The original 65 debug transitions and the panel press arrived in
order. During the withheld acknowledgement the independent reader received 31
notifications. Stop released both held buttons in 3.11 ms and the slot became
free. The fixture passed in 40.08 seconds with no cleanup errors.

The follow-up used the same native executable and compiled publication as
above, and the committed `3893eb7b` frontend in the native WebView. The
Raw Input probe SHA-256 was
`5ed395550241f052ac39fff604c347e5d0fe90efd97e8ba2df9b4f47b5d86947`.
Evidence is in `.research/dcs-physics-native/1794039524953/evidence.json`.
An earlier attempt timed out while detecting the inspector. The first
DevTools run reached the vJoy phase but used an older Raw Input probe that
exited before reporting readiness. The recorded pass uses the current
dual-button probe.

## Authored UDP to browser canvas timing

The timing publication adds two compiled lamp frames with distinct colors.
Each of 24 alternating ownship values was timestamped at the first UDP
submission carrying that value. The production browser panel recorded the
matching fresh host feedback and the actual canvas write of the expected
color. The test aligned the Node and browser clocks before and after the
sample. A native telemetry observer held its first acknowledgement throughout.

| Stage | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Authored UDP submission to browser worker delivery | 30.83 ms | 48.69 ms | 49.02 ms |
| Authored UDP submission to canvas write | 31.23 ms | 49.19 ms | 49.52 ms |
| Browser worker delivery to canvas write | 0.50 ms | 0.60 ms | 0.60 ms |

Clock alignment uncertainty was 0.18 ms. The p99 is the maximum of the 24
samples. The test gates the p95 upper bound at 150 ms. The fixture publishes
ownship at 40 ms intervals and measures from submission on loopback. The
measurement ends at the canvas write and covers one panel. LAN delivery,
display scan-out, simulator export and the full simultaneous R0 workload
remain separate qualifications.

The full rerun passed in 41.11 seconds with no cleanup errors. After the
fixture process exited, the same generation-2 profile owner remained active
in Degraded with only game route and telemetry notices and no fault. The
native executable and Raw Input probe identities match the preceding run.
Evidence is in `.research/dcs-physics-native/1794040014188/evidence.json`.
The publication hash is `sha256:e2e2deb75ccfb202722302ea4ca9491d02a5bca3a09982276e648ba88114621a`.
[Timing publication](../../scripts/fixtures/compiled-flight-timing-publication.json)

The first run measured 49.15 ms p95 to the canvas, then reported a mapping
heartbeat expiry from the final Stop after process exit. The feeder fallback
released ownership, while recovery guards rejected independent cleanup. That
failure is preserved in `.research/dcs-physics-native/1794039894784/evidence.json`.
The rerun adds explicit owner and fault snapshots before and after process
exit. The intermittent Stop fault remains open.
