# Profile-owned application status

App no longer polls independent DCS/integration runtime status. Passive unified
profile status is bounded to visible relevant views. Overview uses the exact
native applied owner/revision separately from saved profile labels and editor
selection. Broker context readiness never claims fresh telemetry. Missing legacy
cleanup metadata is shown as unavailable rather than inferred healthy.

Game metadata removes old profile/default-profile/session DTOs. Installation,
provider settings and cleanup stay separate from active ProfileSession status.
Manual action controls show native profile ownership blockers instead of inferring
bound outputs from obsolete DCS/integration configuration.

Companion compaction reads strict saved unified publication references. Missing,
malformed or read-only reference responses skip both local compaction and native
prune. The native retention fence is described in
`docs/profile-companion-retention.md`.

Validation: 64 profile workspace/component cases, 11 game launch cases, 30 profile
SDK cases and 29 game helper cases pass. Profile SDK coverage is
100%/99.81%/99.04% lines/branches/functions; game helpers are
81.90%/96.44%/96.47%, preserving their existing thresholds. Both Svelte checks
report zero errors/warnings and both frontend builds pass. The Edge launch
fixture passes with 14 intercepted IPCs. Five actual Overview/App/compaction
handlers were rerun after removing the obsolete detected-process PID assumption.
All browser native responses are fixtures; hardware/game qualification is separate.
