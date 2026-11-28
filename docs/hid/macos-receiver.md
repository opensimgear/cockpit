# macOS IOHID receiver qualification

The native Cockpit uses the Tauri app entry and Apple's IOHID APIs. The HID gateway is
invoked from the running app's WebView through `window.__TAURI_INTERNALS__`; it does not
use Linux's WebKitGTK HTTP inspector. Each qualification run selects one current interface
ID from `controller_hid_list`. The selected registry entry ID, exact report descriptor
bytes and IOHID element identities must be checked against a separate IOHID inventory
process. App callbacks and the independent inventory are separate evidence; neither is a
USB bus trace.

The maintained independent probe [`probe-macos-elements.swift`](../../tests/hid-device-captures/probe-macos-elements.swift)
reads the selected device's registry identity, report descriptor property and parsed
elements. It does not call `IOHIDDeviceOpen`, request a report, read a feature, or write a
report. Its element cookies and parent cookies are IOHID session identities, not portable
HID descriptor offsets. Its descriptor digest applies to the exact bytes returned by
`IOHIDDeviceGetProperty(ReportDescriptor)`.

## Prepare the native host

Use a macOS host with Xcode command-line tools, Swift, Rust, Node 22+, and the project
dependencies. Project configuration currently runs native CI on `macos-14`; tagged desktop
packages use `macos-15` arm64 and `macos-15-intel` x64. Those hosted machines provide native
build/package coverage, not attached controller evidence. This qualification requires a
Mac with the intended physical controller connected. Run the same regular user session for
Cockpit, Safari and the independent probe so macOS device-access restrictions remain visible.

Build the read-only probe and list current IORegistry IDs:

```sh
mkdir -p tests/hid-device-captures/.generated
swiftc tests/hid-device-captures/probe-macos-elements.swift \
  -framework IOKit -o tests/hid-device-captures/.generated/probe-macos-elements
tests/hid-device-captures/.generated/probe-macos-elements --list
```

Select the intended controller by its exact `registryEntryId`, vendor/product, transport,
usage and product name. Do not select by name alone. Preserve the independent receipt:

```sh
tests/hid-device-captures/.generated/probe-macos-elements \
  --registry-id 4294967297 \
  > tests/hid-device-captures/.generated/macos-probe.json
```

Replace the sample decimal ID with the current device's exact ID. Re-run the probe if the
device reconnects; a registry ID is a current OS address. A changed ID is a new observation.
The probe rejects missing/oversize report descriptors and oversized or empty element lists.

## Capture through the running app

Start a development native Cockpit (`pnpm tauri dev`) or open the macOS package under test.
In Safari, enable the Develop menu if needed, then select the Cockpit WebView from Develop.
Use this helper in its JavaScript console:

```js
const hid = async (name, fields = {}) => {
  const request = { command: `controller_hid_${name}`, clientId: 'macos-hid-qualification', ...fields };
  const result = await window.__TAURI_INTERNALS__.invoke('gateway_request', { request });
  if (!result.ok) throw new Error(result.error);
  return result.state.controllers;
};
```

Start with `await hid('list')`. It provides bounded inventory summaries; select exactly one
current `interfaceId`, then run `await hid('inspect', {interfaceId})` and retain the returned
identity, `registryEntryId`, `descriptorHex`, `descriptorSha256` and `elements`. Compare them
with `macos-probe.json`; do not use VID/PID, product name, location or cookie alone as a
substitute for matching the exact registry entry and descriptor. If the row is restricted,
inaccessible or metadata-only, preserve that response as an access result and do not open it.

Check `profile_status` first and require a null owner. Open only the selected interface with
`await hid('open', {interfaceId})`; retain the returned generation. During the observation
window, poll `await hid('reports', {interfaceId, generation, after})` and advance `after` to
the last returned sequence. Preserve every delivery in order. `iohid-input-report` with
`wireAvailable:true` is raw callback evidence; `iohid-element` with `wireAvailable:false`
is a parsed element callback and must not be described as raw report bytes. Record report ID,
timestamp, values and source provenance as returned. Close with
`await hid('close', {interfaceId, generation})` and retain the close acknowledgement.

Make the physical action log while observing the device directly: first hold all controls
still, then press and release one identified button, move one identified axis away from its
observed starting position and return it, and operate one identified hat if present. Record
the control label, action, observer, approximate time, and the corresponding app sequence or
element cookie. Keep each observation literal: it establishes only the described change and
return, not calibration, full travel, every control or device-output behavior. Do not induce
keyboard, pointer or restricted-system input. No test transition or fixture value is physical
evidence.

Repeat with the app window unfocused or minimized while the selected reader remains open.
Record whether callback sequences continued, paused or ended and the exact terminal/gap
status. This checks the observed background state on this host; it does not establish OS
behavior under every power, lock-screen or App Nap condition.

For reconnect, close the current generation, disconnect only the selected device, and keep
the retired receipt. Reconnect it, re-list, re-inspect and run the independent probe again.
Open the newly discovered interface as a new generation. Record whether the registry ID,
interface ID, descriptor or element set changed. A late report from the old generation must
not be attributed to the replacement. This procedure qualifies only the observed removal
and re-enumeration; it does not authorize automatic selection by the old ID.

## Verify separate receipts

Save app results as a normalized JSON object and identify the exact app build inspected:
`appBuildType` is `development` or `package`. Preserve development and package receipts in
different files. Required app fields are `fixtureEvidence:false`, `deviceOpenedByRunner:true`,
the selected `interfaceId` and `inventory.interfaceId`, `identity.registryEntryId`,
`descriptorHex`, `descriptorSha256`, full `elements`, retained `reports`,
`physicalObservations`, and `closed:true`. Each physical observation includes `control`,
`action` (`press`, `release`, `move`, `return`, `disconnect` or `reconnect`), `observedBy`
and `appEvidence`. Do not merge receipts from separate app runs or generations.

Run the offline validator from the Cockpit root:

```sh
node tests/hid-device-captures/verify-macos-receiver.mjs \
  --app tests/hid-device-captures/.generated/macos-development.json \
  --probe tests/hid-device-captures/.generated/macos-probe.json
node --test tests/hid-device-captures/verify-macos.test.mjs
```

The validator checks exact registry entry, descriptor bytes/digest and IOHID element
identities/metadata, bounds and callback provenance. It intentionally reports
`packetSynchronized:false`: equal values, timestamps or raw bytes across separate readers
do not establish packet-by-packet timing. Its output always has `releaseQualified:false`.
An element-only callback run can pass only as element evidence, never as raw-report evidence.
Run the same capture against the packaged app and retain a second `appBuildType:"package"`
receipt when that package's WebView can be inspected. If the packaged WebView is not
inspectable, report package runtime qualification as `not-run`; successful CI compilation
or DMG creation is package-build evidence only.

Keep candidate receipts under ignored `.generated/`; this workflow does not replace archived
fixtures. A zero-device list, software probe fixture, app unit test or hosted CI run remains
software/build evidence and cannot be promoted to physical hardware qualification.
