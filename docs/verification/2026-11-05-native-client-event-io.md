# Native companion client event I/O

This delivers the native client read/cancellation slice of #195, with acquisition
accounting in #189 and qualification in #198. It does not close those issues.

## Delivered behavior

The client's 100 ms read timeout is replaced by the existing nonblocking Mio
transport. Each operation or passive reader has a bounded, epoch-scoped registry
of cancellation controls. Disconnect, replacement, Forget and runtime shutdown
cancel registered I/O directly. Socket registration after cancellation fails
closed. Registry retirement cancels outside its mutex and cannot retire a newer
epoch. Feedback retries and pending-pairing waits are also cancellation-aware.

Request deadlines cover both send and reply, including TLS writes; scoped guards
restore the previous deadline. Passive feedback waits for bytes without an idle
poll. Its transfer budget starts after the first complete WebSocket frame.
Presence waits until its actual two-second keepalive deadline and releases the
strong runtime reference before waiting. Partial first-frame reads remain
cancellable. Existing frame/transfer bounds remain enforced.

Credential selection, worker reservation and status publication recheck the
current epoch. Successful Forget advances the epoch again at its durable commit,
rejecting credentials or worker reservations selected during deletion. Exhausted
epochs fail closed without wrapping. Regression tests cover delayed invalidation,
blocked credential deletion, stale selections and maximum-epoch cancellation.

## Local verification

- 230 companion service, 2,289 native Cockpit, 46 companion serial and one platform
  test pass; the native suite retains 17 existing explicit ignores.
- All 39 production companion coverage gates pass the 71% floor. Client: 93.20%;
  scoped I/O: 96.80%; feedback: 92.94%; presence: 90.10%; deadline I/O: 100%.
  All 17 coverage-parser tests pass. All 247 pinned timing sites retain exactly
  one disposition.
- Locked offline Windows production Cockpit and native library probe builds pass.
- Six actual-Cockpit tests pass: native library pairing/presence/cancellation;
  listener observation/retry; sixteen stalled server TLS peers; delayed preview;
  physical-source reader replacement/quiet liveness/Stop; and compiled browser
  WSS input/profile/feedback/reload/Stop.

The actual Cockpit process was PID 42444, using isolated runtime
`.research/reactive/mcp-integration-receipt-runtime`. Executable SHA-256:
`2507c6e488b5124f977f9052ea866e6971879bc9fb7a505363f2c8496616f07f`.
No desktop timing audit was enabled. Native client evidence is under
`.research/reactive/companion-client-io-native`; coverage is under
`.research/reactive/companion-client-coverage-final`.

The native library test uses pinned pairing, isolated Windows-protected storage,
three presence/worker epoch handoffs and 6.5 seconds of quiet keepalives. A real
TCP peer deliberately stalls TLS. Disconnect completes in 57.9 ms in this single
run, before the three-second handshake deadline; Forget closes passive presence.
This is software cancellation evidence, not physical latency or a percentile.
Final listener is disabled with zero active connections; the profile is stopped.

To reproduce the library probe, copy `tests/fixtures/native-client-io-probe.rs`
to `crates/companion-service/examples/client_io_probe.rs` in an isolated checkout.
Build with `cargo build --locked --offline --manifest-path
crates/companion-service/Cargo.toml --example client_io_probe`. Set
`OSG_NATIVE_CLIENT_PROBE` to that executable, `OSG_NATIVE_CDP_URL` to the running
development Cockpit's CDP endpoint and `OSG_COMPANION_CLIENT_IO_NATIVE=1`, then run
`node --test tests/ui/companion-client-io-native.mjs` with its listener disabled.

## Corrected checks and remaining boundaries

Initial fixture compilation needed the new scoped-connect argument. The first
live fixture used an unsupported `stop` client command; it was corrected to the
existing public `disconnect` command and passed. No public command was invented.

An attempted desktop build of the mobile shell reached its explicit Android/iOS
guard. That guard was preserved. Mobile authentication is intentionally disabled
pending protected-store/lifecycle prerequisites #55/#56; the Windows library
probe does not qualify the mobile shell. Android/iOS pairing, backgrounding,
reconnect and cancellation remain separate acceptance in #199 after prerequisites.

DNS resolution and TCP establishment precede socket registration and retain
their existing two-second bounds. Cancellation checks before each connection
attempt and registration fencing prevent late admission, but these phases are
not claimed immediately interruptible. Pending pairing still checks approval
every 400 ms; making its wait cancellable does not remove that acquisition poll.
Legacy companion routing/firmware loops, full per-widget expiry, slow-consumer
and lifecycle matrices remain original software work. Physical input latency,
real simulator effects and platform acceptance remain unqualified. CI remains
deferred until implementation is complete.
