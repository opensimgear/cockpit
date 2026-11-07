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
