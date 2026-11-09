# Applied virtual preview visibility

The applied virtual controller preview registered its visibility handler only
if the document was visible when the editor mounted it. Opening the editor
while hidden left it in Preview after the document became visible.

The component now registers its visibility handler whenever it has a saved
profile revision and is active. That handler opens the runtime reader only
while visible. Sample ownership still follows the exact applied owner and
revision.

## Local verification

The focused component test failed on the original code because it never
registered the visibility handler. The running Windows Cockpit also timed out
waiting for Live and a pressed button after the first reveal. Both checks passed
with the fix.

The fixture uses an isolated database and an authored Boolean source routed to a
real vJoy slot. It sends a press before the first reveal and a release while
hidden again. It observes the matching applied sample on each reveal and
checks retirement on hide and on navigation out of the editor.

| Transport | Initially hidden sample opens | Total opens / exact retirements | Runtime inspection requests | Slot free after Stop |
| --- | --- | --- | --- | --- |
| Native WebView | 0 | 2 / 2 | 0 | Yes |
| Browser-dev | 0 | 2 / 2 | 0 | Yes |

Both runs kept the same profile owner across visibility changes. The frontend
was based on `6d31b12b` with the visibility fix. Native used the dev
`osg-cockpit.exe` (SHA-256 `0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`).
Browser-dev used the directly linked gateway binary (SHA-256
`2272c6b0035edb1470da3155b733c9b3a6bbccd14d996660a41b353fdba66242`).
All 1,706 frontend tests passed and Svelte reported zero diagnostics.

This fixture qualifies the applied preview lifecycle. Physical controller
latency is tracked in [#199](https://github.com/opensimgear/cockpit/issues/199).

## Visible preview timing on both transports

After the second reveal, the fixture sent 64 alternating debug-controller
button transitions through the running Cockpit. It waited 50 ms after each
acknowledgement and observed frame before sending the next transition.
The applied profile routed each button state to vJoy slot 1. The selected
button stayed in the preview viewport. The page's monotonic clock recorded
each command submission, the matching pressed-class mutation and the next
animation frame.

| Transport | Submission to preview DOM p50 / p95 / p99 | Submission to next render opportunity p50 / p95 / p99 |
| --- | ---: | ---: |
| Windows native WebView | 4.60 / 5.20 / 6.40 ms | 4.60 / 18.70 / 21.70 ms |
| Windows browser-dev | 6.00 / 7.20 / 8.00 ms | 6.10 / 18.90 / 23.60 ms |

Both transports matched all 64 transitions, kept one subscription during
the visible burst, made zero runtime inspection requests, and freed the
vJoy slot at Stop. In the preceding successful pair, next-frame p95 was
18.30 ms native and 19.80 ms browser-dev. One browser-dev sample reached
57.90 ms. All four measured runs met the 100 ms foreground p95 budget.

The frontend came from the isolated `af78067d` source copy. The virtual
preview component and gateway client matched main. Native used the
`f993800a` dev executable, SHA-256
`69f274fa6795682b90f34604b0c1c91bebb3b8893c08289a69ffa9ccf5a4ab81`.
Browser-dev used the Windows gateway executable with SHA-256
`47dd58fa0da1ebabed61e3e3e9cfb3174b33a25c005cfb6ff4c2bc165abe9361`.
The isolated profile IDs were `8fbe9a70-3943-4d6c-a6cd-09784aa200ae`
and `dac28da6-e02c-4b26-a629-a2575bc4bc5c`, both at revision 2.

Artifacts are under `.research/reactive/virtual-preview-reopen/`.

| Trace | SHA-256 |
| --- | --- |
| `native-timing-final.json` | `c2f0dbf521df9b002d5b59bfcfe940d49f1cd9844eb183f0e8cc16201d1126ef` |
| `browser-timing-final.json` | `bae14451790bca73974b1c0316ea49b8f3160263293b1afcdade6f2291c662eb` |

Both live fixture runs and 13 focused frontend tests passed. The changed
fixture measured 100% lines, 81.69% branches and 91.18% functions on native
and 98.42%, 76.71% and 91.18% on browser-dev. These measurements cover
command submission through applied preview presentation. Physical
controller capture, Windows vJoy report receipt and display scanout each
require separate timestamps.

## Paired preview and Windows Raw Input receipts

The fixture now verifies each acknowledged debug-controller sequence and
records the matching preview mutation, next frame opportunity and Windows
Raw Input receipt for the selected vJoy slot. It pairs all 64 transitions
in order and requires each receipt to fall before the following input under
the full browser-to-QPC clock bracket. The receiver remains active through
the last frame. This uses authored software input.

| Transport | Submission interval p50 / p95 / p99 | Submission to vJoy Raw Input upper bound p50 / p95 / p99 | Submission to DOM p50 / p95 / p99 | Submission to next frame p50 / p95 / p99 |
| --- | ---: | ---: | ---: | ---: |
| Windows native WebView | 122.10 / 122.60 / 122.60 ms | 1.80 / 2.13 / 2.72 ms | 4.60 / 5.60 / 5.60 ms | 4.80 / 19.70 / 22.30 ms |
| Windows browser-dev | 121.90 / 126.20 / 136.90 ms | 2.11 / 2.49 / 11.56 ms | 5.60 / 6.50 / 15.30 ms | 5.80 / 17.00 / 21.20 ms |

The median offer rate was about 8 Hz in both runs. The fixture waits for the
previous frame and then paces the next input. The peak pending input queue was
one frame per transport. The QPC brackets were 0.77 ms and 0.88 ms wide.
Both runs met the 20 ms p95 and 50 ms p99 destination targets from the earlier
command submission boundary and the 100 ms p95 preview target. Neither run
queried runtime inspection, and each retired both subscriptions and freed the
slot at Stop. Physical capture and scanout still require their own timestamps.

Both native executables use `ec7348da`. The isolated `af78067d` frontend
contains the same applied preview and gateway code. The current Raw Input
probe was built from `tests/controller-mapping-m0/RawInputProbe.cs`.

| Local artifact | SHA-256 |
| --- | --- |
| Native executable | `42797065eaf7cf2a185168af58808904690cf896517157524e93eb51a067857a` |
| Browser-dev gateway executable | `7cd535cca53574c3d6410027a6f808e497ab0246c18bb8732454aa573fc653ae` |
| Raw Input probe | `6ea93864c16d2897aedc150df5addf45fd65a3dcd89a0f40773a779c88f35e9e` |
| `native.json` | `d6bc00f3270535228a3c8c9f2674f3b41fe92ab1064643aeece11b283b9c1091` |
| `browser.json` | `fdc1368416ddd4aa10d75692518ca5d5113a44f96d56b1fdda009cd0dd28be37` |

The traces and coverage reports are under
`.research/c193-reconnect/applied-preview-paired/`. The Raw Input comparison
helper reached 93.26% lines, 84.62% branches and 89.47% functions on both
transports. The native preview-only mode also passed with the receiver
disabled. Six adjacent dual-rate and Raw Input timing tests passed.

## Paired Windows preview and vJoy receipts under dual-rate load

The running Windows dev Cockpit used native revision `c8a57cb0`, clean
frontend `88ee864e`, and a private profile `cbfb0c78-0181-43d6-9cc4-a05229ad5536`
at revision 2. The qualification source offered 7,201 transitions at
1,000.02 Hz and 901 at 125.00 Hz to buttons 3 and 4 of the same virtual
controller. During the 7.2-second overlap, the foreground preview and the
Windows Raw Input receiver each matched the same 64 authored button-1
transitions in order. The 0.76 ms clock bracket bounded the receipt timing
and confirmed every measured preview frame fell within both load lanes.

| Boundary | p50 / p95 / p99 |
| --- | --- |
| Button-1 submission to vJoy Raw Input, upper bound | 1.75 / 2.22 / 2.65 ms |
| Button-1 submission to preview DOM | 5.60 / 7.10 / 12.20 ms |
| Button-1 submission to next frame opportunity | 14.40 / 21.30 / 24.10 ms |
| 1 kHz source capture to reducer batch end | 0.20 / 0.30 / 0.30 ms |
| 125 Hz source capture to reducer batch end | 0.20 / 0.30 / 0.30 ms |

Peak pending input was two frames. The preview retired both subscriptions,
reopened on current state, and issued zero runtime-inspection reads. Stop
freed the vJoy slot. The background lanes were paired through reducer batch
completion. Exact Windows receipt was measured for button 1. The fixture
used software input and measured the next browser frame opportunity.
Physical controller capture and display scanout remain in [#199](https://github.com/opensimgear/cockpit/issues/199).

| Local artifact | SHA-256 |
| --- | --- |
| `launch-child-wake.exe` | `4933f3a9c9bea2da4106484d24248e39f644095676473091109b7076a7ad4050` |
| `preview-load-27236.log` | `3534b01df3361a39b1d584090a0cd7f9ca94128f1b892ed6b87440b2f2863555` |
| `qualification-fast.bin` | `43907c93fef93ec80c7fa531407091d51bf70098e3d0fbb7b3601e3f938731cc` |
| `qualification-peer.bin` | `535157382f752a59fd97f0794c1a6bc21330558893c97dd75fbe243bc6af38e0` |
| Private `profile-virtual-preview-reopen.mjs` | `b6cd2ad2343a703e07262fd75eb5ef53a7c44207ef5ad2997c28aee1c0b5fe33` |

The log and `preview-load-trace-27236` directory are under
`.research/c193-reconnect/dcs-native-stage-timing/`. The Raw Input probe
has the same fingerprint as the earlier paired run.
