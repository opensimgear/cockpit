# Verify the receiver under the pointer before mixed-load timing

The native fixture previously proved the foreground keyboard receiver and
client-relative cursor coordinates. Windows can route mouse messages to a
different window under those coordinates. The receiver now records the exact
window and process under the pointer and the foreground capture state. The
latency fixture uses a receive-only mode that leaves the cursor alone until
it finds an unobstructed client area on its own foreground window. It checks
the target again after warmup, before the pointer workload. The receipt gate
requires a marked `WM_MOUSEMOVE` in the exact foreground receiver without
mouse capture. [Windows mouse routing](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-mouse-input)

In the initial diagnostic run, `PickerHost.exe` PID 17540 owned window
2755484 under the pointer while the receiver owned keyboard foreground.
The recorded overlay bounds and the virtual desktop both measured
2560×1440 from (0,0). Raising only the test receiver and sampling its client
area still found no unobstructed region. The fixture reports the specific
obstruction and keeps the receiver available for orderly teardown.

Final run `r4-desktop-c3526cafdb` used Cockpit `08142f5b` with executable
SHA-256 `6cd86e880e2b8c6a95b7c2d688d33991b6447d58462b496a7e930e898d93295f`
and receiver SHA-256
`8178c417374c57c88a9c3362dba29969c739e6a5ce1cdc5c356ba12532154b68`.
The preflight stopped with the same overlay target. The audit contained zero
native input submissions. The receiver exited normally and both cleanup
checks reported no errors. Local evidence is in
`.research/reactive/r4-desktop-timing/run-16c007dd/`.

All 29 portable desktop tests passed. The receiver build and the native
obstruction and cleanup path were exercised locally. Mixed-load mouse
receipt and UI-load timing require an unobstructed desktop. This is a test
environment prerequisite for #193 and #198. Physical controller and preview
acceptance remain in #199.

## Current desktop preflight

A read-only recheck at Cockpit source `5d05deda` confirmed Windows could
read the cursor. PickerHost PID 17540 owned both the foreground window
and the pointer target. The hit-tested window was still HWND 2755484,
covering the 2560×1440 desktop. The preflight stopped the mixed run before
launch. It submitted no keyboard or pointer input. The record is
`.research/c193-reconnect/required-journal-leaf/current-desktop-preflight.json`,
SHA-256 `19fa0e650bf6d9514aa49e3156a6b65396bbad08dbb5b92b93cdbcac3ae5cb6c`.
