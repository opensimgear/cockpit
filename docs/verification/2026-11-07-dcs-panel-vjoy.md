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
