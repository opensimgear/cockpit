# Keyboard and mouse output contract

This is the Windows J0/J2 contract for feature [#97](https://github.com/opensimgear/cockpit/issues/97). `crates/mapping` owns portable action scheduling and trace time; `crates/desktop-output` owns shared native ownership, worker transport, native submission and interrupted-session records. Unified Profiles remains the authoring and activation boundary. There is no desktop profile store or selector.

## Capability boundary

Saving, importing, recording, editing and ordinary startup never start native output. Local enablement defaults to false. Production `osg_desktop_output::QUALIFIED` remains false pending J6. A Windows debug build compiled with `OSG_DEV_DESKTOP_INPUT=1` permits explicit development acceptance; this does not qualify a release or a game. The private same-executable worker is checked before constructing the GUI. Desktop-only activation does not load or inspect vJoy/HidHide. A mixed profile must independently pass each selected backend's gate.

## Physical keys and ownership

Keys are USB HID keyboard-page usages, not characters. The supported initial set is `04–47`, `49–65` and `E0–E7`. All eight modifiers retain their left/right identities. Pause (`48`, requiring E1 encoding), media/consumer controls, Unicode text and unsupported usages are refused. The Windows adapter sets `KEYEVENTF_SCANCODE`, the explicit extended flag and `wVk=0`; it never substitutes a character. Physical Y therefore types `y` on US and `z` on German layouts. Right Alt is an extended physical modifier, including German AltGr behavior provided by Windows.

Native ownership is keyed by run session/generation, node and action. Dependencies retain exact source generations/epochs. A producer must be armed from a fresh released/neutral baseline. Its held vector replaces only its own contribution. All routes share one reducer; the last Cockpit owner releases an OS key/button. Native scan-code aliases (HID `31`/`32`) share a ledger entry. Modifiers press before other keys and release after them; ties have a stable physical-key/button order. A producer cannot release another producer's Control key or mouse drag. A tap colliding with another hold reports the collision and does not manufacture an up/down edge. Explicit repeat adds key-down records while retaining ownership.

These are Cockpit ownership guarantees. Windows physical keyboard state shares the OS stream with injection, so an injected release cannot be isolated from a physically held key. `GetAsyncKeyState` is never used as evidence of ownership. Physical overlap still requires native qualification and visible product explanation.

## Scheduling limits

| Operation | Initial policy |
| --- | --- |
| Held key, chord or mouse button | Desired contribution, no unchanged-state replay |
| Tap/click | Ordered down/up dwell; default 30 ms down and 30 ms up; per-action queue bounded to 32 taps |
| Repeat | Explicit profile delay/interval; skip missed intervals, never catch up in a burst |
| Pointer | 8 ms scheduler interval (125 Hz target); elapsed-time clamp 50 ms; retain fractional motion and clear it on pause; actual host cadence requires qualification |
| Wheel | Ordered horizontal/vertical steps; each unit is one Windows `WHEEL_DELTA` |
| Native batch | At most 64 native `INPUT` records; both wheel axes count separately |
| Native held ledger | At most 64 distinct native controls; at most 1,024 armed producers |
| Worker transport | 64 KiB frames, 32 queued input/output frames, 100 ms parent response budget |
| Worker lease | 250 ms; watchdog checks every 5 ms independently of UI/input arrival |

The scheduler receives explicit monotonic trace time and performs no clock reads. The coordinator translates remaining mapping deadlines onto the worker's native monotonic clock; it never compares clock origins. An expired pointer/repeat is skipped with a diagnostic. An expired held action releases that producer. Cleanup remains eligible after an action deadline, scope loss or lease expiry. Sequences/rotations remain the separate J5 scope.

## Native submission and failures

One private worker holds `Local\\OpenSimGear.DesktopInput.v1`, excluding a second Cockpit desktop/browser-service injector in the same Windows session. Each worker request uses a contiguous desktop channel sequence, independent of controller reports. Deliveries retain their operation ID, exact producer token, source dependencies and source timestamp/deadline. Stale tokens, replayed operations, gaps, oversized frames and unsupported keys are refused.

Before input dispatch, the parent atomically writes and syncs a bounded in-flight journal containing both acknowledged and possible held controls. The worker retains desired/attempted state separately from confirmed submission. `SendInput`'s returned count must equal the native batch count. A short count or worker death after submission before ACK is uncertain: fault, attempt bounded releases, retain the parent record, require explicit recovery, and never replay a click or shortcut. A stale ACK cannot clear the record. Lease renewals do not introduce new input and avoid journal fsync on every heartbeat.

Journals are regular local files bounded to 128 KiB on both write and read. Recovery validates the journal's run/generation, owner tokens, in-flight producer and captured application scope against the ProfileSession receipt. Missing or corrupt records never establish successful cleanup once Start could have injected. A successfully acknowledged worker Stop is terminal and idempotent, including repeated Stop during owner Drop; reactivation or recovery acquires a fresh coordinator.

Worker EOF, parent loss, lease expiry, Stop and foreground loss attempt owned cleanup. They do not promise cleanup when Windows terminates the whole process tree, the desktop becomes inaccessible or the worker is killed before cleanup. An interrupted record blocks activation. Explicit recovery releases only its validated recorded possible controls and remains stopped. Startup does not send releases or resume actions.

## Foreground eligibility

Default scope is the selected application. Desktop-wide scope is a separate explicit choice. Application eligibility binds executable path, PID and `GetProcessTimes` creation identity; PID reuse does not inherit a scope. Every batch and watchdog pass checks foreground identity, interactive `Default` desktop availability and target integrity. Unknown identity, focus loss, a locked desktop or a higher-integrity foreground target pauses/refuses output. Returning to the application requires a fresh neutral baseline and never resumes a partial action. Normal Cockpit startup remains unelevated.

`SendInput` has no target window. A focus switch between eligibility checking and API submission remains an OS race and must be measured during qualification.

## Evidence boundary and current results

The portable/worker recording tests cover shared modifiers, native aliases, focus/lease cleanup without input, fresh rearming, deadline skipping, replay rejection, transport bounds, session exclusion, partial return counts, durable pre-dispatch journal inspection and simulated worker death after submission before ACK. These tests prove reducer/protocol behavior, not native application delivery. Process/pipe transport is a separate module from coordinator ownership/journaling, and native acceptance below exercises that process boundary.

The explicit receiver command is:

```powershell
cargo run --offline --manifest-path crates/desktop-output/Cargo.toml --example native_receiver -- --accept-native-input docs/verification/2026-10-18-desktop-native-receiver.json
```

The [redacted native evidence](verification/2026-10-18-desktop-native-receiver.json) records 112 batches: 124 requested and 124 returned native records, with 152 receiver window messages on US (`00000409`) and German (`00000407`) layouts. Receiver characters include US `y`, German `z`, German AltGr+Q `@`, extended keypad Enter and the ISO physical key. Five mouse buttons and both wheel directions were observed. The sustained 125 Hz pointer experiment submitted 64 records; 62 marked movement messages were observed, illustrating why native submission count and receiver messages are different measurements. Character messages generated by `TranslateMessage` need not preserve the injected marker.

The successful experiment ran as an ordinary unsandboxed user process. A preceding restricted process attempt returned a short native submission and was cancelled with its tracked holds released. This is not higher-integrity target evidence. The receiver probe measures direct API calls, excluding controller capture, graph routing, durable coordinator writes, worker IPC and application/game processing. It does not establish p95/p99 end-to-end latency.

The [same-executable coordinator/worker evidence](verification/2026-10-18-desktop-worker-receiver.json) additionally uses the real private worker, durable journals and native receiver. It proves shared Control ownership across two producers, five button clicks, both wheel axes and motion through IPC; actual focus loss to a separate process releases native holds and refuses resume until a new Arm; no input/UI renewal for 250 ms releases the held key; actual parent termination exits without Drop and the worker independently releases on EOF; and an actual debug worker termination immediately after successful `SendInput` but before ACK leaves one observed key-down, no release and an in-flight possible-hold record. A second delivery is refused without replay. Explicit journal recovery releases that recorded hold and leaves the journal stopped. Interrupted parent records also block activation even after the worker successfully cleaned up, preserving the distinction between native observation and parent knowledge.

Run this separate explicit experiment with a Windows debug acceptance build:

```powershell
$env:OSG_DEV_DESKTOP_INPUT = '1'
cargo run --offline --manifest-path crates/desktop-output/Cargo.toml --example native_receiver -- --accept-worker-input docs/verification/2026-10-18-desktop-worker-receiver.json
```

The test-only `OSG_DESKTOP_TEST_EXIT_AFTER_SEND=1` hook is compiled only in debug builds, requires the explicit acceptance gate, and exits only after a submitted delivery before its ACK. Ordinary tests never invoke native injection. Receiver windows explicitly take foreground focus and stay topmost so click tests land within their client area; this focus management is limited to the opt-in test utility. `--receiver-only <output.json>` provides a five-minute non-injecting receiver for running-Cockpit acceptance, writing live observations every 50 ms. `<output>.focus` and `<output>.stop` markers (replacing the output extension) request focus or shutdown.

The [worker lifecycle regression evidence](verification/2026-10-18-desktop-worker-lifecycle.json) runs three actual same-executable worker cycles without submitting any input. Each excludes a concurrent injector, acknowledges Stop, proves a second Stop leaves the clean journal unchanged, drops the worker transport and immediately reacquires the native session lock. This covers profile replacement and repeated owner cleanup independently from receiver input acceptance. Invoke `--accept-worker-lifecycle <evidence.json>` with the same debug acceptance build.

The [running-Cockpit companion evidence](verification/2026-10-18-desktop-companion-receiver.json) exercises authenticated WSS inputs through actual Tauri IPC, the unified applied profile, scheduler, coordinator and private worker. Two controls retain shared Control ownership; twenty encoder edges in one WSS write produce ten serialized Enter taps; one axis update sustains pointer motion; companion lease loss releases keys and a mouse drag and stops motion. A separate native process receives cleanup releases after scope loss; returning while the trigger remains held does not resume it, and a fresh release followed by a press rearms it. Both receivers exit, no injector worker remains, and the profile and enablement finish stopped. These are receiver and native status observations, not physical-controller or game evidence.

The [actual ACR scope evidence](verification/2026-10-18-desktop-game-native.json) launches the installed game through Cockpit's manual profile selection, binds its exact native process and creation identity, acknowledges a Right Arrow submission while foreground, observes cleanup in a separate receiver on focus loss, requires fresh neutral on return, and preserves the running Right Arrow action after saving a Left Arrow draft. Stop while held cleans the journal, and the test closes only its verified newly launched game. This proves game process/foreground integration and native submission; game consumption and displayed response were not measured.

The [staged package worker evidence](verification/2026-10-18-desktop-package-worker.json) uses the existing development packaging script to copy the unchanged executable, then starts its private worker from the staged directory three times with empty stdin. Each emits only its bounded hello and exits on EOF; sequential startup proves the session lock is released. No protocol commands are sent and no input is requested. This covers staged development startup, not installer/upgrade or packaged input qualification. Reproduce with `scripts/desktop-package-worker.ps1 -Executable src-tauri/target/debug/osg-cockpit.exe -OutputDirectory .research/<new-probe-directory>`.

The [whole development app interruption evidence](verification/2026-10-18-desktop-crash-native.json) terminates the exact private worker and application tree while the receiver holds KeyA. Ordinary restart exposes RecoveryNeeded without sending input or starting a worker. Explicit recovery releases exactly the recorded KeyA once and leaves the session stopped with a clean journal. The test runs only with both `OSG_DESKTOP_CRASH_NATIVE=1` and `OSG_DESKTOP_NATIVE_INJECT=1`.

Still unverified: ordinary versus higher-integrity target acceptance, physical-key overlap, real controller/panel delivery, actual unplug/publication replacement, sleep/wake and lock/unlock, installed package input/deployment, sustained loaded-system latency and representative game behavior. The [qualification record](verification/2026-10-18-desktop-output.md) separates actual running-Cockpit companion acceptance from these remaining J2/J4/J6 checks; production enablement remains closed.
