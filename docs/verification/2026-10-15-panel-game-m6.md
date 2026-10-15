# M6 panel-event and game-profile verification — 2026-10-15

Status: software implemented for ordered panel recording and schema-2 captured game mappings, including optional output/visibility coordination. Issue #32 remains open; it is In progress during implementation and validation, then In review for physical qualification. Live virtual output and visibility coordination remain disabled, pending the platform qualifications in #25–#29 and real DCS/racing-game acceptance.

- Independent nonconsuming consumers preserve short presses, baseline on stream/boot loss, reject unavailable axes/hats and duplicate gateway routes, and cancel owned subscriptions on Stop. Native adapter tests use the same contract validator as DCS-BIOS, with separate mapping eligibility.
- Real mapping-service/game-coordinator tests cover immutable capture, preflight before hardware/launch, failed-launch neutralization before restoration, fresh-baseline resume, emergency Stop, schema-2 restart with explicit recovery, external run conflicts, and explicit abandonment without stopping an external run. Existing schema-1 and integration ownership tests remain enabled.
- Mapping-host production coverage after lifecycle completion: Linux 38 tests and Windows 37 tests pass; learn 94.19%, router 91.27%, settings 97.62%, panels 100%, game coordination 88.66%/88.70%, and service 79.73%/72.77% (Linux/Windows). The measured summaries are in `.research/mapping-host-coverage/summary.json` and the Windows local coverage directory.
- Native integration coverage includes each new module, rather than excluding the adapter: final panel adapter 92.48%, game schema/overlap 97.92%, games coordinator 92.47%. Final results are in `.research/m6-integration-coverage/integration-summary.json`.
- Windows desktop regression passed 298 tests with three existing hardware-dependent tests ignored. Linux game regression passed 28 tests, including actual schema-2 journal reopening. Frontend mapping helper coverage is 100%; game helper coverage exceeds 70%. Final full CI is recorded on the issue after push.
- The running Windows Tauri dev app exposed five connected Pico panel-event sources. Read-only inspection returned button controls with `hatsSupported:false`. This confirms app/adapter wiring, not vendor/firmware qualification.
- Native WebView UI created a temporary game profile, loaded a saved mapping, captured it through the schema-2 upgrade button and saved it. Readback retained the expected mapping ID; original default game profiles were unchanged, no mapping was started and qualification flags remained false. The temporary profile was deleted. No JavaScript exceptions were recorded. Screenshot: `.research/controller-mapping-m0/cockpit-m6-game-capture.png`.

No vJoy layout, HidHide policy, hardware configuration, game launch or existing game-profile default was changed during native UI validation. Physical simultaneous consumers, unplug/replug, actual DCS/racing output, qualified visibility setup/release and driver-level failure cleanup remain open evidence items. Schema-2 requested virtual/hiding operations fail before effects until those paths qualify.

## Optional lifecycle completion after implementation audit

Captured game runs now use their immutable target bindings and explicit captured
visibility policies, independently of later editor settings. Legacy captures
remain readable; a visibility capture without explicit policies must be recaptured
before activation. The game selector directs isolated Linux launch to its separate
Devices launcher instead of suggesting that a Windows game session can supply
that boundary.

Preflight checks qualification, opt-in, source policy, output dependency readiness,
Windows slot shape/availability, script compilation and interrupted ownership
before the coordinator changes hardware. The game journal reserves an owner;
the output journal uses the same owner. Output is acquired in fallback/neutral
state and its lease renews without mapping reports while visibility verifies
fresh capture. Only then are fresh input/panel baselines and compiled workers
armed. Emergency Stop invalidates an in-flight start.
Restoring a prior recording preview preserves recording mode even when that
profile's optional requirements request virtual output or hiding; it never gains
new hardware effects during resume.

Visibility uses an internal coordinator API, separate from frontend IPC. It
never overwrites standalone preferences. The mutating worker rechecks the exact
preflight policy before applying; normal/inverse policy constraints remain those
of the existing adapter. Windows has a durable journal per game owner; pending
records are reported for explicit game recovery and block another activation.
Linux/macOS suppression stays capture-owned and ephemeral. Recovery releases
owned state; it does not recreate grabs, restart mappings or run scripts.

The router supervises visibility independently of UI polling. Visibility loss
stops output; output failure requests visibility restoration. Normal game stop
neutralizes/releases output and waits for owned visibility restoration before
the coordinator may restore hardware. Conflicting policy, settings or output
ownership is preserved with an error. Existing schema-1/2 restart and failed
launch ordering tests remain enabled.

New tests use the production feeder protocol with authored driver adapters,
real isolated Rhai workers, actual visibility journal/worker logic and injected
physical registries. They cover all four combinations of optional requirements,
neutral preparation, continuous supervision, external conflicts, explicit restart
recovery and Stop during activation. These software checks do not qualify real
driver/game cleanup. The physical criteria stay unchecked and the production
output, visibility and script qualification flags remain false.

Local validation includes Linux/Windows controller coverage (game visibility
92.91%), Linux virtual-output coverage (session 91.84%), portable runtime coverage
(92.72%), mapping-host coverage above 70% per module, 28 Linux game-coordinator
regressions, and Svelte/type checks with zero errors or warnings. Final native
results and exact-commit CI are recorded on issue #32 after push.
