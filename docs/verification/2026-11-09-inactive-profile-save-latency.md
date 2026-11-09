# Windows virtual-controller latency during an inactive profile save

Running Windows dev Cockpit routed two native debug input lanes to separate
buttons on one vJoy slot. Their measured rates were 1,000.02 and 125.00 Hz in
the second trial, with 7.20 seconds of overlap. The editor renamed and saved
a different profile from revision 1 to 2. The active run kept the same owner,
revision and generation. Each trial delivered all 8,102 ordered edges and
both Stop releases to the Windows Raw Input receiver.

| Trial and lane | Offer to receipt p50 / p95 / p99 | Maximum offer to receipt | Maximum dispatch to admission |
| --- | ---: | ---: | ---: |
| 1, 1 kHz | 0.4 / 0.6 / 7.1 ms | 60.28 ms | 57.67 ms |
| 1, 125 Hz | 0.7 / 0.8 / 2.9 ms | 57.52 ms | 56.49 ms |
| 2, 1 kHz | 0.4 / 0.6 / 1.2 ms | 51.89 ms | 50.83 ms |
| 2, 125 Hz | 0.7 / 0.8 / 3.0 ms | 51.54 ms | 49.00 ms |

The aggregate percentiles mask a repeatable stall. In trial 2, the Save click
and acknowledgement were 99.49 and 239.23 ms after the scheduled workload
start. Within that interval, padded by 2 ms on either side when aligning with
the first recorded offer, receipt p95 was 47.14 ms for 144 fast offers and
51.54 ms for 18 peer offers. The worst fast offer spent 0.02 ms before
dispatch, 50.83 ms before admission and 1.05 ms from admission to receipt.
The worst fast offer in trial 1 spent 57.67 ms from dispatch to admission
and 2.60 ms from admission to receipt.

The editor takes `profile_admission` for writing while normalizing,
validating and persisting a profile, including an inactive profile. Debug
input admission takes the same lock for reading before entering the
coordinator. The native physical controller capture path uses its own
report watch. This run covers authored debug admission and vJoy receipt.
Physical controller input, preview frames and display scanout require
separate traces. The next software check is to let debug admission proceed
during unrelated persistence while keeping owner replacement and saved
revision checks atomic, then rerun with a save-window timing assertion.

The first fixture asserted cached slot availability immediately after Stop.
The corrected second trial checked the native release separately from the
cached status. Both releases reached Raw Input within 2.55 ms of Stop and
the inventory showed the slot free by 296 ms. Cleanup reported no errors.

The native binary includes the watched-game launch change from `c8a57cb0`
and has SHA-256
`4933f3a9c9bea2da4106484d24248e39f644095676473091109b7076a7ad4050`.
The frontend is from `88ee864e`. The Raw Input probe SHA-256 is
`0a66dca7c5355cafe6e1bc12fc52e00e06af9fda9b16baa1c27eea6f44a0bee5`.
The local trial traces are in `.research/c193-reconnect/current-ui-88ee864e/source/.research/profile-save-dual-rate/`.

| Trial directory | `evidence.json` SHA-256 |
| --- | --- |
| `d89b971e-68ae-4626-bdad-cf02710568b6` | `5592a4d2ad94188004c28d93d9ae2d31fb819529b745a1fa97f3119308e38076` |
| `30b5a019-f5a4-4393-b75e-b57d93730d7b` | `4b0eef3605bc2d5ef6264ea451f24fbd429b278c1eb4189d63d20295aa50008a` |

## Shared admission for inactive saves

An editor save now checks the running owner under shared profile admission.
It holds that guard through an inactive save, so the classification stays
valid through persistence and the active run's debug input can enter
concurrently. An active save obtains exclusive admission and checks
the owner again. Revision checks and the profile-store mutex still govern
competing writes.

The same running Windows dev fixture measured exactly the offers between
the Save click and acknowledgement. The baseline failed the fixture's
20 ms maximum dispatch-to-admission check. All three candidate runs passed. Each
trial matched all 8,102 offers and both Stop releases with an unchanged
active owner. The first two candidate runs peaked at two pending frames in
each lane. Both Stop releases reached Raw Input within 2.81 ms, and the
inventory showed the slot free by 304 ms. The runs cleaned up without errors.

| Trial | Maximum dispatch to admission, 1 kHz / 125 Hz | Maximum offer to vJoy receipt, 1 kHz / 125 Hz |
| --- | ---: | ---: |
| Baseline | 62.19 / 54.90 ms | 64.48 / 55.91 ms |
| Candidate 1 | 0.243 / 0.090 ms | 0.607 / 0.670 ms |
| Candidate 2 | 0.141 / 0.103 ms | 0.683 / 0.666 ms |
| Final executable | 0.301 / 0.085 ms | 0.642 / 0.751 ms |

The first candidate executable SHA-256 is
`2da382d2b5b1f7846610dbbe31090371b5c14e45514373fcf3e2ff4a53c73123`.
The final executable SHA-256 is
`0a722a56cb4b1726d516245bbb7f925114c2c37e455fd7c6fcb4df0f8a3d1c47`.
The frontend and probe are the same as above. This isolates the save
stall for debug input routed to vJoy. Physical input and on-screen preview
latency remain separate acceptance in #199.

The gateway regression held shared admission while the inactive editor
save committed, then checked the same active owner and the new durable
revision. The final Windows suite passed 2,499 tests with 18 existing
ignores, including the active editor save and replacement tests. A first
run with eight workers hit disk-full errors while X-Plane fixtures copied
the test executable. The two-worker runs passed those tests. LLVM measured
93.97% lines, 82.35% functions and 90.79% regions for the editor-save
module. This build did not instrument branches. All 11 focused JavaScript
timing and trace tests passed.

| Trial directory | `evidence.json` SHA-256 |
| --- | --- |
| `aa16f78e-da5c-4071-9bc7-20f4891e23cb` | `dddcb615e757a016e1234ed7c1594c35d02c9542df34ce590a862a2d3ad75305` |
| `20537682-ff54-409b-a24f-0baa2263470e` | `eebf855174e9c59446a0ea40cfc1b8b3bd4abf541c796b44f3905ebcb4d07f84` |
| `d7962d68-aa4f-464c-87d7-0eac152a18cc` | `fa99a336b1720660e1261024c210794fb80f34879622276b52d039a5f228c784` |
| `2bf33cc8-fbe9-4fcf-b809-ef44b94887eb` | `9369abb270ec94d3fda5496288a5d860a55a3ad6c66cfecda6a17e51ae84c682` |

## Foreground preview during inactive saves

The native save fix cleared admission for the 1 kHz and 125 Hz background
lanes. A second test exposed a separate wait in the WebView. The shared
frontend gateway serialized debug input behind profile authoring requests
before submitting it to Cockpit. It now gives debug input and status their
own ordered queue. The Debug controller inspector uses the typed gateway
directly.

Each run sent 64 authored button transitions through the frontend gateway
with the preview visible. Of these, 16 were submitted between the start
and acknowledgement of an inactive save. Each save advanced the draft
by one revision.
The preview stayed open and the applied owner stayed the same. Native
trials also admitted 9,601 fast and 1,201 peer offers across 9.6 seconds.
The clock bracket places every measured preview frame within that load.
The browser-dev trial used the real gateway service with the preview and
saves as the workload.

| Trial | Save samples to DOM p50 / p95 | Save samples to next frame p50 / p95 | Save samples to vJoy upper p50 / p95 | Frames before save acknowledgement |
| --- | ---: | ---: | ---: | ---: |
| Before both fixes | 54.3 / 76.7 ms | 57.3 / 86.0 ms | 49.43 / 73.02 ms | 0 of 16 |
| Native fix only | 67.3 / 73.3 ms | 73.4 / 83.2 ms | 63.61 / 69.01 ms | 0 of 16 |
| Native and frontend fixes, run 1 | 5.8 / 6.3 ms | 14.6 / 21.6 ms | 1.58 / 1.98 ms | 16 of 16 |
| Native and frontend fixes, run 2 | 5.8 / 6.4 ms | 15.4 / 22.4 ms | 1.65 / 2.24 ms | 16 of 16 |
| Final frontend | 5.9 / 8.1 ms | 15.8 / 21.7 ms | 1.62 / 2.50 ms | 16 of 16 |
| Browser-dev, both fixes | 5.5 / 6.3 ms | 5.6 / 20.2 ms | 1.81 / 2.12 ms | 16 of 16 |

With 16 samples, p95 and p99 both report the maximum. Every run paired
all 64 preview edges with their exact Windows vJoy Raw Input receipts.
The preview retired and reopened with current state, issued zero
runtime-inspection requests and retired on navigation. Stop freed the
slot. This measures the next browser frame opportunity for authored input.
Physical controller capture and display scanout remain in #199.

The original 7.2-second workload ended before the last baseline frame.
The reruns extended both the background and receiver windows and required
full overlap. The first run with only the native fix failed its vJoy
budget as shown above. The frontend queue regressions passed for native
IPC and browser-dev HTTP, including ordered press, release and status.
All 158 transport tests passed. Gateway coverage measured 100% lines,
96.72% branches and 89.29% functions. Cockpit and browser companion
Svelte checks reported zero errors or warnings. The widget and checked
fixture commands also passed when run directly.

The older full authoring fixture stopped during setup. A focused running
Cockpit test opened the real Debug inspector from a private saved profile,
pressed and released its Button 1 and verified both native states. Stop
then caused the previous owner to be rejected.

The native executable is the final build above. The browser gateway
service SHA-256 is
`a7164c3d58fe28a90c5707d82c9b2f4fd82247141f32565c8241534340ce96e8`.
The Raw Input probe SHA-256 is
`6ea93864c16d2897aedc150df5addf45fd65a3dcd89a0f40773a779c88f35e9e`.
The logs are under `.research/c193-reconnect/dcs-native-stage-timing/`.

| Trial log | SHA-256 |
| --- | --- |
| `preview-save-full-baseline.log` | `aa42af8df4e16056fb892b2a44bf69a77433dadae8ae5f2687516abc329937e0` |
| `preview-save-full-candidate-1.log` | `d6f827ce291ff4f17e9d9ad69780f625f47a881497ca421044895307b593c2ce` |
| `preview-save-full-gateway-1.log` | `026d003e0a0fd6767741288c08ccb6608cc75e6c3f944245d1b6c1762753ba87` |
| `preview-save-full-gateway-2.log` | `0baa875459dc74c20cddb74056eb3533ec8f43255023691f704a7f46b50b86aa` |
| `preview-save-exact-final.log` | `5402f2b0b6f64d05b26d1ffcde46cfdc1b64abb3282fc8106fbecb9db56c9889` |
| `preview-save-browser-dev.log` | `a4eaeecba3c69a12f925fddeedfe9c6256deb584e85721216242cc22fe4a3bb2` |
| `preview-save-debug-control-focused.log` | `90b908359e6c850849519e886883949940f47b6c2c9a1c91adb4c2bc1ca50b17` |
