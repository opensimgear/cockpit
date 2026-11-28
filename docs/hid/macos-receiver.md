# macOS IOHID receiver qualification

The native Cockpit uses the Tauri app entry and Apple's IOHID APIs. The HID gateway is
invoked from the running app's WebView through `window.__TAURI_INTERNALS__`; it does not
use Linux's WebKitGTK HTTP inspector. Each qualification run selects one current interface
ID from `controller_hid_list`. The selected registry entry ID, exact report descriptor
bytes and IOHID element identities must be checked against a separate IOHID inventory
process. App callbacks and the independent inventory are separate evidence; neither is a
USB bus trace.

The maintained independent probe [`probe-macos-elements.swift`](../../tests/hid-device-captures/probe-macos-elements.swift)
looks up the selected IOHID service in the IORegistry and reads its registry identity,
report descriptor property and parsed elements. It creates an `IOHIDDevice` view of that
service but does not call `IOHIDDeviceOpen`, request a report, read a feature, or write a
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

Start with `await hid('list')`. It provides bounded inventory summaries, including each
row's shared-open and process listen-event access status. Select exactly one current
`interfaceId`, then run `await hid('inspect', {interfaceId})` and retain the returned identity,
`registryEntryId`, `descriptorHex`, `descriptorSha256` and complete `elements`. Compare them
with `macos-probe.json`; do not use VID/PID, product name, location or cookie alone as a
substitute for matching the exact registry entry and descriptor. If the row is restricted,
inaccessible or metadata-only, preserve that response as an access result and do not open it.

Paste this bounded capture function after selecting the exact interface ID. Set
`appBuildType` to `development` for the dev app and `package` for the packaged app. It
checks that profiles are stopped, inspects before opening, collects one reader for at most
10 seconds, preserves report order and closes the reader even after a failed check. The
function returns the complete normalized app receipt as JSON in Safari's console; save it
separately from the independent probe receipt.

```js
async function captureMacHid(appBuildType, interfaceId, durationMs = 2000) {
  if (!['development', 'package'].includes(appBuildType)) throw new Error('Set the app build type.');
  if (!Number.isInteger(durationMs) || durationMs < 100 || durationMs > 10000) throw new Error('Duration must be 100..10000 ms.');
  const clientId = `macos-hid-${crypto.randomUUID()}`;
  const request = async (command, fields = {}) => {
    const payload = { command: `controller_hid_${command}`, clientId, ...fields };
    const value = await window.__TAURI_INTERNALS__.invoke('gateway_request', { request: payload });
    if (!value.ok) throw new Error(value.error);
    return value.state.controllers;
  };
  const profile = await window.__TAURI_INTERNALS__.invoke('gateway_request', {
    request: { command: 'profile_status', clientId }
  });
  if (!profile.ok) throw new Error(profile.error);
  if (profile.state?.profiles?.profileSession?.owner != null) throw new Error('Stop the active profile before qualification.');
  const inventory = await request('list');
  const rows = inventory.interfaces.filter(row => row.interfaceId === interfaceId);
  if (rows.length !== 1) throw new Error('The exact current interface ID must match one inventory row.');
  const detail = await request('inspect', { interfaceId });
  if (detail.interfaceId !== interfaceId || !Array.isArray(detail.elements) || detail.elements.length === 0) throw new Error('The selected interface has no complete element inspection.');
  const descriptorHex = typeof detail.descriptorHex === 'string' ? detail.descriptorHex.toLowerCase() : null;
  const descriptorAvailable = descriptorHex !== null;
  if (descriptorAvailable && (!/^(?:[0-9a-f]{2})+$/i.test(descriptorHex) || descriptorHex.length > 131072)) throw new Error('Invalid or oversized report descriptor.');
  const descriptorSha256 = descriptorAvailable
    ? [...new Uint8Array(await crypto.subtle.digest('SHA-256', Uint8Array.from(descriptorHex.match(/../g), byte => parseInt(byte, 16))))].map(byte => byte.toString(16).padStart(2, '0')).join('')
    : null;
  if (descriptorAvailable && detail.descriptorSha256 && descriptorSha256 !== detail.descriptorSha256.toLowerCase()) throw new Error('Native descriptor digest does not match its bytes.');
  const opened = await request('open', { interfaceId });
  if (opened.interfaceId !== interfaceId || typeof opened.generation !== 'string') throw new Error('Native reader did not return the selected generation.');
  const reports = [];
  let retainedBytes = 0;
  let after = '0';
  let closeAck = false;
  try {
    const deadline = Date.now() + durationMs;
    while (Date.now() < deadline) {
      const page = await request('reports', { interfaceId, generation: opened.generation, after });
      const current = page.result;
      if (current.gap || current.terminal != null) throw new Error(`Capture gap or terminal status: ${JSON.stringify(current.terminal)}`);
      for (const report of current.reports) {
        const size = new TextEncoder().encode(JSON.stringify(report)).byteLength;
        if (reports.length >= 4096 || retainedBytes + size > 8 * 1024 * 1024) throw new Error('Capture evidence reached its bound.');
        retainedBytes += size;
        reports.push(report);
        after = report.sequence;
      }
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    if (reports.length === 0) throw new Error('No callback was retained in the observation window.');
  } finally {
    const closed = await request('close', { interfaceId, generation: opened.generation });
    closeAck = closed.result?.closed === true;
  }
  if (!closeAck) throw new Error('Native reader did not acknowledge close.');
  return {
    origin: 'running-macos-tauri-webview', appBuildType, generatedAt: new Date().toISOString(),
    fixtureEvidence: false, deviceOpenedByRunner: true, interfaceId, inventory: rows[0],
    identity: detail.identity, descriptorAvailable, descriptorHex, descriptorSha256,
    elements: detail.elements, generation: opened.generation, reports,
    retainedBytes, closed: closeAck, operatorObservations: []
  };
}
```

Run it with the selected `interfaceId`, for example
`await captureMacHid('development', 'INTERFACE_ID_FROM_LIST')`. The output includes raw
callbacks as `iohid-input-report` with `wireAvailable:true` and parsed element callbacks as
`iohid-element` with `wireAvailable:false`. The latter must not be described as raw reports.
No report GET or write is needed. Descriptorless inspection can retain raw callback bytes,
but their projection must say unavailable; it cannot claim portable decoding. Element-only
callbacks remain parsed-value evidence without raw bytes. If no callback arrives during an
idle window, record no reception; do not synthesize a value to make the run pass.

If physical actuation is available, separately record what the operator observes: first hold
all controls still, then press and release one identified button, move one identified axis
away from its observed starting position and return it, and operate one identified hat if
present. Record the control label, action, observer, approximate time, and corresponding app
sequence or element cookie. Keep each observation literal: it establishes only the described
change and return, not calibration, full travel, every control or device-output behavior.
These manual notes are optional and are never counted as an automatic receiver check. An idle
capture can qualify reception while leaving physical transitions unqualified. Do not induce
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
`descriptorHex`, `descriptorSha256`, full `elements`, retained `reports`, and `closed:true`.
Optional operator notes live separately under `operatorObservations`; each includes `control`,
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
