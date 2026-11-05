# Native pending pairing events

This implements the pending native pairing acquisition cutover in #195 and
updates the #189 timing disposition. It does not close the broader milestones.

## Behavior

After the initial pending reply, the native client sends one `pair-subscribe`
frame on its existing pinned TLS/WebSocket connection. The host sends the
terminal approval or error when the pending request changes. The 400 ms
`pair-status` acquisition loop is removed. Manual and serial `pair-status`
requests remain used by their active command workflows.

Admission validates the exact frame, request ID and constant-time secret hash
under the Hub mutex. At most 16 readers exist, with one reader per pending
request. Weak notification handles and exact RAII identity prevent an old
reader from removing its replacement. Approval grants no input or connection
authority; the client must authenticate separately. The reader verifies the
approved token against the retained credential before returning it.

Approval, rejection, cancellation, revocation, credential replacement and expiry
wake the corresponding reader. Reading the snapshot after observing the wake
generation closes the registration/approval race. The only timed wait is the
original offer expiry; wakes cannot extend it. Disconnect/Stop retire even
unauthenticated readers. The native client's existing I/O scope cancels its
blocking subscription request and the absolute operation budget covers writes
and the terminal reply. The client verifies the returned request identity before
credential persistence.

## Verification

- 238 companion service tests and 46 serial tests pass. Six new Hub tests cover
  admission, capacity, pre/post registration approval, expiry, denial/cancellation,
  revocation/rotation, Stop and exact cleanup. Two new native transport tests
  cover a quiet TLS peer with no acquisition frames for 650 ms before approval,
  and production listener rejection, cancellation and Stop without credentials.
- All 41 production companion coverage gates pass the 71% floor. Pairing reader:
  100%; socket handler: 80.77%; client: 93.80%. The minimum gate is 73.61%.
- All 2,289 native Cockpit tests pass, with 17 existing explicit ignores. All
  17 coverage-parser tests and 247 pinned timing dispositions pass.
- Locked offline Windows Cockpit and native service-library probe builds pass.
  All six actual-Cockpit checks pass: native library pairing/presence/cancellation,
  listener observation/retry, sixteen stalled TLS peers, delayed controller
  preview, source-reader replacement/quiet liveness/Stop, and compiled browser
  input/profile feedback/reload/Stop.

The actual Cockpit was PID 38888 in isolated runtime
`.research/reactive/mcp-integration-receipt-runtime`, with desktop audit disabled.
Executable SHA-256:
`39cff6413d6a760a8e56a3f2b812cbabae9f351392ae48550ce973027dd135cb`.
Coverage and logs are under `.research/reactive/pairing-event-*`. Final listener
is disabled with zero connections/paired clients; the profile is stopped.

The Cockpit UI offer auto-approves its native pairing workflow. Therefore its
live check proves the existing pairing/presence behavior remains healthy; the
pending wait and terminal transitions are proven separately by real TLS tests
against the production server and native client, rather than claimed exercised
by that auto-approved UI path.

## Corrected checks and boundaries

The initial all-bin build failed to overwrite the running
`osg-gateway-service.exe` with Windows access denied. Building the required
`osg-cockpit` binary explicitly passed; the unrelated running gateway was left
alone. After the browser test passed, a cleanup query incorrectly included a
companion field in `profile_status`. The correctly shaped read-only query
confirmed the final stopped/empty state; no product command was retried.

A remote peer closing during the server's passive wait can retain its reader
until the original offer expiry, at most 60 seconds. Capacity remains bounded;
listener Stop wakes the reader directly. This is not claimed prompt remote-close
detection. Mobile authentication/protected-store prerequisites #55/#56 and actual
Android/iOS lifecycle acceptance remain open, as do companion slow-consumer,
publication/per-widget expiry and physical transport qualification. Software
prerequisites remain in the original issues; user-operated acceptance is #199,
first in Progress. CI remains deferred until implementation is complete.
