# Windows companion reconnect and desktop pointer recheck

## Companion reconnect

The Windows offline-companion fixture now requires the observed online
source to match a current Hub assignment and publication before it
publishes a writable route. During concurrent unassignment, the
collector can still see the previous provider descriptor. In that case
it falls back to the paired metadata path with no writable authority.
This matches the production inventory's fail-closed behavior. The
change is confined to the native test fixture.

The isolated candidate at `87e25238` with the test patch applied passed
2,486 Windows native tests with zero failures and 18 existing ignores.
The formerly intermittent test passed in the full suite and in 20
consecutive isolated runs. The native test executable SHA-256 is
`2c4e44670542e3804024d8d73f09509470c377501c2c08b786a07b6b883bf67f`.
The suite log is at `.research/c193-reconnect/native-suite.log` and
has SHA-256 `90bd6e07649b268e4a6c52a46ce80fef416a3b946209d572e39e7548fcae87f7`.

The existing compiled-browser test also passed against the running
Windows dev Cockpit, executable SHA-256
`b1eb29dd1c65e128d4084d9bb9584e74b042c33b50c7727dea99f93ba4635966`.
The browser reloaded on the same applied owner, started from neutral,
and routed a new press to live feedback. The log is at
`.research/c193-reconnect/live.log`.

## Mixed keyboard and pointer

On the same Cockpit executable, the guarded fixture again found window
2755484 from PID 17540 under the pointer while the private receiver had
keyboard foreground. This is the [previously identified PickerHost
window](2026-11-09-desktop-pointer-target.md). The test stopped before any
native input submission. The wrapper verified clean ownership and
reported zero cleanup errors. The preflight evidence is in
`.research/reactive/r4-desktop-timing/mixed-recheck-3e7a338f7c/evidence.json`
with SHA-256 `6eb2b4192e84fa6fd1df7411752b2d1a3679d76c9cdd09d9c2a7d7bca3dae84b`.
Mixed pointer receipt needs an unobstructed desktop. The strict
dispatch targets remain open.
