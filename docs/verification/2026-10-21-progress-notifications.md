# Profile progress notifications

Settings → Progress notifications now selects System notifications or Cockpit
window delivery and one of four window corners. Defaults are System notifications
with a bottom-right fallback. Preferences are saved natively; changing the corner
moves an already visible Cockpit notification. Operating-system placement remains
under Windows control.

Automatic vehicle changes now publish native progress with the immutable profile
and vehicle names, actual preparation steps and terminal outcome. Base-only
fallbacks identify Base. Switching is noncancellable; manual activation retains
its existing Cancel behavior, and output cleanup remains noncancellable.

## Running dev Cockpit checks

The Windows dev app was rebuilt and tested through its actual native IPC,
WebView2 windows, persisted settings and runtime. The repeatable display acceptance
test is `tests/ui/profile-progress-notifications-native.mjs`, enabled with
`OSG_PROGRESS_NOTIFICATIONS_NATIVE=1`.

- The System preference attempted the real Windows notification API. This dev
  installation returned `Element not found (0x80070490)` when checking its own
  notification permission, so the actual Cockpit fallback was exercised through
  six updates. A successful Windows banner was not observed or qualified.
- Window delivery, exact-operation close protection, actual Cancel intent and
  subsequent noncancellable cleanup passed. Preferences were restored afterward.
- All four corners passed, including moving an already visible window. At 150%
  display scaling the client measured 630×144 physical pixels (420×96 logical),
  with a 24-pixel inset inside the 3840×2088 monitor work area. Native window focus
  remained false. Screenshots were visually inspected.

The complete `profile-vehicle-switch-native.mjs` acceptance test also passed:
Hornet → Viper, stale telemetry → Base, recovery → Hornet, and an unconfigured
aircraft → Base. Each transition checked the actual native notification, selected
graph nodes and UDP samples. Stop prevented subsequent reactivation. Fixture
profiles, provider settings, navigation and notification preferences were cleaned
up. An earlier run timed out on the final Base UDP sample; a rerun with unchanged
assertions passed. These authored RREF packets and the inert X-Plane process
exercise Cockpit's real transports, not an actual simulator flight. Actual DCS
aircraft qualification remains documented separately in
`2026-10-21-dcs-actual-aircraft.md`.

The Windows backend checks notification permission, Do Not Disturb and session
suppression, and falls back on rejected or missing updates. Other platforms use
the Cockpit window until their system notification backends are implemented.

## Automated checks

- Svelte checks: zero errors and warnings for both frontends.
- Both frontend builds and the native debug build passed.
- 42 focused frontend tests and 149 profile workspace tests passed.
- 48 focused native automatic-selection, progress, window and OS-notification
  tests passed.
- 67 gateway transport/workspace tests and three native admission regressions
  passed. Live diagnostics identified automatic Connect cancelling initial
  profile preparation before an owner existed. Background Connect/Start now defer
  during preparation; explicit controls retain their cancellation authority.

Local evidence and screenshots are under `.research/profile-progress-notifications-native/`.
