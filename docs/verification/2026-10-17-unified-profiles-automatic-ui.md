# Automatic profile selection UI — 2026-10-17

Game details now contain a saved unified-profile rule with an enabled flag and
signed 16-bit priority. Choices come from the unified Profiles service. Current
installation and saved native executable identity are read-only; browser writes
never contain an executable, PID or process session. A missing installation or
removed profile stays explicit. Existing rules can be disabled or removed, and
enabling requires native process-monitoring support. Native validation remains
responsible for saved-profile eligibility and actual process verification.

The Profiles list has explicit Manual/Automatic controls and displays the native
manual pin, automatic target, conflicts and policy status separately from the
applied runtime snapshot. Opening an editor, creating a draft, changing settings
navigation, refreshing status and disposing components issue no mode or Apply
request. Automatic controls are not added to the editor. Legacy game-hardware
profiles remain separate pending their planned migration.

The game rule form preserves newer local edits while a captured Save is in
flight. A late response after disposal cannot replace its UI state. Read-only or
stale native status blocks writes; malformed status arrays are rejected instead
of being coerced into valid enum labels. Status polling is read-only and runs
only while the component is mounted, active and visible. Profile choices are
loaded independently without refreshing a retained profile editor store.

The six new tests execute actual Svelte SSR, production handlers and lifecycle
functions with the typed native-shaped client/controller. They cover passive
opening, explicit mode-only requests, native pin/target/conflict rendering,
exact rule Save/Remove, stale installation handling, profile/priority guards,
newer-edit preservation, late replies and visible polling. The game-details and
Profiles workspace tests also verify the actual component integration. This is
SSR/handler evidence; physical/game automatic-activation qualification is a
separate native/runtime acceptance boundary.

| Check | Result |
| --- | --- |
| Combined workspace and runtime SSR/handler suite | 42 passed |
| Game-provider integration SSR suite | 4 passed |
| Automatic client independent coverage | 6 tests; 83.33% lines, 98.15% branches, 100% functions |
| Automatic controller independent coverage | 4 tests; 100% lines, 89.36% branches, 100% functions |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Browser and desktop production UI builds | Passed; desktop 251 modules |
