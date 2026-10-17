# Unified Games launch controls

Games tiles and details use `GameProfileLaunch`, which sends only the logical
game ID to native `profile_game_launch`. The old per-game profile editor,
peripheral captures and alternate Activate/Launch controls are removed. Provider
settings, installation selection and explicit unfinished-session recovery remain.

Stop and passive status use independent native requests. Late launch replies
cannot replace a newer Stop, changed executable/game or disposed component.
Successful completion requires both the bounded native launch acknowledgement
and an actual running/degraded profile status with matching owner/applied profile.
Hardware-only and other-game selection are displayed as returned; unavailable
branches remain visible. Detail/tile remount retains an available Stop control.

`pnpm game-launch:test` passes all 11 actual component/App SSR and handler cases.
Both Svelte checks report zero errors/warnings; desktop/browser frontend builds
pass. Existing profile workspace and provider UI checks remain passing.

`tests/ui/profile-game-launch.mjs` runs headless Edge against the existing dev
frontend. It covers keyboard launch, responsive Stop, late completion fencing,
detail/tile navigation, degraded status, independent provider settings and mobile
main-content layout. Every native IPC is intercepted with authored responses;
HTTP fallback is rejected and no second service/hardware owner is started.
Screenshots are retained under `.research/profile-game-launch-browser/`.

These are frontend fixtures. Native process ownership has separate Manager tests;
actual Windows/game/device/companion acceptance remains separately recorded.
