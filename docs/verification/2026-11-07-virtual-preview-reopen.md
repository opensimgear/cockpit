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
