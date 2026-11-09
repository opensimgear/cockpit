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
