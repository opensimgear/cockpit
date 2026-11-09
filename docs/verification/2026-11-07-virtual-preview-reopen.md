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
