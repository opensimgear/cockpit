# Offline peripheral activation

Assigned physical controllers, OpenSimGear modules and companions can be absent
when a profile activates. Their branches, settings and hide intents stay inactive;
independent branches run and the native profile reports Degraded. The header and
overview show Degraded explicitly, with a warning naming each offline peripheral.
Invalid assignments, changed retained contracts, ambiguous routes and unknown
provider failures retain their existing gates. Skipped peripherals acquire no
input subscriptions, output claims, panel assignments or visibility leases.

When the same native identity returns with matching contracts and an available
route, the running immutable profile is prepared again through the existing
owner-fenced handoff. Successful recovery clears its offline warnings. Repeated
inventory polls do not replay activation, and Stop or newer manual controls win
over pending recovery. Whole gateways continue to use their existing connection
mechanisms; this does not authorize independent device writes during ownership.

Confirmed gateway disconnects retain known module identities and contracts while
revoking their routes. This also works at startup before an in-memory observation
cache exists. Connected gateways with unreadable inventory or capabilities remain
unavailable rather than being treated as a known disconnect.

## Running dev Cockpit acceptance

The final native Windows debug build and actual frontend were exercised using a
disposable production browser companion and the retained asset-free `flight-panel`.
`tests/ui/profile-offline-peripherals-native.mjs` passed in 27.8 seconds with
`OSG_OFFLINE_PERIPHERALS_NATIVE=1`.

- Paired a real browser, assigned the retained panel and captured its exact live
  boolean port contract. Closing its page retained that identity as Offline.
- Applied a saved profile with that required companion and an independent Base
  constant. Apply succeeded, Base remained known at 100, the peer input remained
  unknown, and no offline companion ownership or assignment changed.
- Verified the visible Degraded header and named overview warnings.
- Reopened the same paired browser context. Native recovery acquired a fresh
  owner/session and real input baseline, returned to Running and removed the
  visible warnings. Further polling retained the same owner generation.
- Stopped the profile and disconnected/reconnected the peer again. No profile
  restarted. The temporary profile and pairing were removed, existing identities
  and selection policy were restored, and cleanup/browser errors were empty.

Screenshots and native evidence are under
`.research/profile-offline-peripherals-native/`. No simulator telemetry was
fabricated and no physical CAN/HID unplug qualification is claimed by this test.

## Automated verification

- 36 compiler tests passed, including offline sources/sinks, unchanged portable
  hashes, contract/backend rejection and inactive hiding/publication policies.
- 118 native runtime regressions passed; two existing hardware-specific cases
  remained ignored. Eight focused offline tests also passed.
- 23 native provider tests passed, covering disconnect/disappearance, startup
  without the observation cache, authority revocation and unknown-provider errors.
- 11 frontend compilation tests and eight overview/header tests passed.
- Both frontend type checks reported zero errors/warnings; frontend and native
  debug builds passed.
