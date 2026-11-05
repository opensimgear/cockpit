# ESP32 terminal pairing events

ESP32 Wi-Fi and direct USB pairing now issue one secret-bound `pair-subscribe`
after the initial `pair-pending` reply. The firmware's repeated 400 ms
`pair-status` requests are removed. The original approval deadline remains;
quiet waiting cannot extend it or activate input.

Wi-Fi uses the existing bounded receiver worker for one terminal reply. Its
primary request socket closes before approval waits, so later authentication
opens a fresh socket after an arbitrarily quiet wait. Cancellation and worker
exit wipe the persistent pairing receive buffer, including partial/in-flight
reads. The worker slot remains occupied until the old worker exits.

The USB bridge acknowledges `pair-subscribed`, retains one native reader and
forwards the terminal `paired` or error reply in its existing `serial-feedback`
envelope. Firmware dispatch validates the USB link before delivery. Reset,
new pairing, authentication, close and cancellation drop the old reader. Exact
request/host/companion checks precede credential acceptance; authentication and
the fresh neutral-baseline handshake remain separate.

Every Hub pairing reader has its own wake identity, even when a caller reuses
its worker wake. Dropping an obsolete reader cannot remove its replacement.
USB and WebSocket terminal delivery sample expiry after acquiring the Hub lock,
so lock contention cannot release an approval using an earlier timestamp.

## Local qualification

The isolated candidate starts at `ce2a1b81`. Production executable SHA-256:
`dde64ce585313c6ae1c51c2dc4b5c3a376e8e568df0a8d2cc9b7854b74939a7f`.
All 1,244 staged native records match the candidate. All 1,231 native coverage
source/test stamps remain unchanged; all 107 recorded unrelated workspace file
hashes are preserved.

- 241 companion-service and 51 serial tests pass. All 43 production coverage
  gates pass, including the complete extracted wire owner at 97.87%, pairing
  owner at 88.89%, and service pairing subscription at 98.59%.
- 2,332 native tests pass, zero failures, 17 existing explicit ignores. The
  native companion host coverage gate passes at 310/340 lines (91.18%).
- Eleven coverage-helper tests, the production Windows build and all 247
  historical timing dispositions pass.
- All six firmware network binaries and nine line/branch/function coverage
  gates pass with AddressSanitizer/UndefinedBehaviorSanitizer, including the
  extracted production pairing fragment as its own coverage gate.
  That fragment has 100% lines/functions and 72.58% branches.
- Actual rebuilt Cockpit native-library pairing, presence handoffs, quiet
  keepalive, stalled-TLS cancellation and Forget pass. The UI's offer is
  automatically approved; this is not evidence of a manual pending-approval
  delay or physical ESP32 execution.
- The real pinned-TLS browser input/feedback/reload/Stop fixture passes, with
  change-driven feedback and no blanket native host service interval.
- Current controller preview recovery, applied profile owner retirement/Stop,
  exact vJoy Raw Input delivery, Activity source wakes and desktop scope lifecycle
  pass. Final audit-disabled runtime is stopped, has no held/possibly-held
  controls or uncertain ownership, and has all virtual slots free.

The firmware software harness covers eight seconds of quiet approval waiting
without another request, fresh authentication socket acquisition, exact expiry,
invalid identities/token, Stop and obsolete links, persistence failure, and
approval without input authority. The actual serial parser forwards raw approval
and error terminals. Receiver tests cover cancellation during a read and before
owner consumption, and assert buffer erasure. These tests do not qualify physical
Wi-Fi, USB serial, flash durability, touch, rendering or board timing.

## Corrections and remaining work

The first firmware run exposed old tests that expected periodic status requests;
they now supply terminal events. Review caught raw USB terminals being filtered,
an idle primary Wi-Fi socket being reused after approval, cancelled credential
bytes remaining in the receive buffer, and expiry sampled before the Hub lock.
Each path was corrected and received a regression. The serial owner was extracted
as a complete logical module so coverage measures the changed transport owner;
the unrelated legacy driver's full-file coverage was only 33.96% in its unit suite.

The pinned ESP-IDF SDK/compiler are absent at the recorded local paths. Board
image compilation remains unqualified, as do physical acceptance and actual
simulator effects in #199. Missing software build/transport prerequisites remain
in #195/#198. CAN ownership notification production, game process/vehicle
maintenance, complete read-model/UI/MCP matrices and integrated qualification
remain in the original #188 scope. CI stays deferred until implementation ends.
