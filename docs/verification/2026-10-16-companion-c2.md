# Companion C2 Windows transport and sources — 16 October 2026

Issue [#40](https://github.com/opensimgear/cockpit/issues/40) implements the Windows
native companion transport and dedicated mapping source for feature
[#37](https://github.com/opensimgear/cockpit/issues/37). It builds on C0 contracts
and the completed C1 schema 2 designer/shared renderer. This record distinguishes
local automated Windows evidence from physical second-PC, mobile and distribution
qualification.

## Delivered scope

- `crates/companion-service` owns a single opt-in Rust WSS listener, native client,
  pairing approval, revocable installation credentials, pinned-host CA trust,
  bounded publications, session ownership and the driver-independent SourceSink
  interface. Each peer has an independent worker; the endpoint cannot invoke
  Cockpit's broad gateway/native commands.
- `crates/mapping-host` retains offline companion descriptors and drains bounded
  per-instance ordered feeds on the existing mapping clock. `Origin::Companion`
  is distinct from HID, panel and Synthetic sources. Schema 1 imports keep their
  meaning; explicit schema 2 adds companion roles. Axis conversion preserves
  exact endpoints, ties-away-from-zero rounding and each declared fallback.
- Cockpit's Companion connections UI exposes listener interface/port selection,
  pairing approval, publication availability, stable instance assignment,
  explicit takeover/disconnect/revoke and status. Mappings can inspect and select
  retained companion controls without frontend input polling.
- The separate Windows Tauri Companion app shares the C1 canvas, uses the Rust
  pinned client, validates downloaded publications/assets, obtains an atomic
  fallback baseline and requires explicit Start. Ordered IPC delivery, native
  epochs, lifecycle cancellation and UI/native/host leases prevent stale replay.

The trust anchor is a persistent self-signed private CA. The listener uses a
separate signed server leaf; the client verifies the CA SPKI pin before sending
secrets and retains ordinary chain-signature, SAN, validity and server-use checks.
The actual advertised endpoint hostname/IP is checked against the leaf SAN;
`hostId` is checked in the authenticated session. No global verifier override,
public-CA installation or plaintext fallback is used.

Windows per-user DPAPI protects host authority/private keys and the client's
credential/trust. Native Android/iOS protected-store adapters are not supplied;
pairing/connect remain disabled there before secret exchange. Linux/macOS support
remains deferred. The listener is disabled on every startup; saved configuration
and offline source identity never resume active input.

Host publications/assets are a bounded memory cache, separate from Cockpit's
persisted IndexedDB designer library. After a host restart the operator must use
Make revision available again for needed revisions. The companion's active
publication is not a durable offline asset cache. Pairing credentials and source
descriptors survive a normal protected-state reload; active sessions do not.

## Local automated validation

Checks ran locally with the Windows Rust/MSVC and Node toolchains, using separate
build/coverage output directories. The final aggregate run records the current
suite totals; no remote CI completion or physical LAN pass is inferred from them.

| Check | Local evidence |
| --- | --- |
| Companion-service unit and real-WSS integration suite | 41 pass: pairing, authority, publication, protected state, real sockets and cancellation/failure regressions |
| Native companion coverage gate | Pass: all eight service production modules exceed 71%; final range 78.89–100% |
| Cockpit companion operator IPC tests | 3 pass; 97.97% production-line coverage in the final run |
| Mapping-host regression suite | 51 pass, including 14 companion source tests |
| Companion provider production coverage | 97.88%; all mapping-host modules exceed 71% |
| Mapping engine/profile regression suite | 40 pass; all measured production modules exceed 92% |
| Frontend mappings suite | 11 pass; 100% measurable lines, branches and functions |
| Frontend bounded publication upload suite | 4 pass; 100% measurable lines, branches and functions |
| Ordered companion IPC pump | 8 pass; 100% lines/functions, 93.75% branches |
| Shared designer/model regression suite | 60 pass, including native canonical/capability compatibility and asset-budget regressions |
| Windows native host C2 compilation and operator IPC | Pass; full host suite 321 passed, one existing test ignored, plus service framing and integration tests passed |
| Companion TypeScript/Svelte check and frontend build | Pass; zero diagnostics in the final run |
| Windows native Companion build and lifecycle-epoch test | Pass; unsigned development executable, two native lifecycle/navigation tests |

Coverage is measured over production lines of each native module rather than an
aggregate that could hide an untested trust module. TypeScript coverage gates
also enforce at least 71% lines, branches and functions for measurable modules.
Svelte rendering, native event delivery, hardware and physical network behavior
are separate from these line counts. The final aggregate rerun includes the ordered
client pump and all native transport, storage and validation hardening.

Real local WSS tests exercise one listener with three clients: two different UIs
and two independent instances of the same UI. They cover atomic baseline/input,
held-input heartbeat renewal, independent source lease loss, UI-stall cleanup,
CA-pin mismatch, wrong authority/SAN, expired leaf and tampered signature,
oversized frames, occupied-port diagnostics and an absolute stalled-handshake
shutdown bound. Publication transfer chunks reproduce canonical bytes exactly.
Protected restart tests retain credential/trust identity while leaving sessions
disarmed. Local loopback peers are not distinct physical computers.

Core regressions reject forged assignments, unknown versions, obsolete sessions,
wrong publications, malformed current input, sequence gaps and overflow. Explicit
host takeover first neutralizes the prior writer, and late cleanup from that
socket cannot cancel a replacement. Independent heartbeats do not fabricate
transitions; a complete 128-control baseline is separate from the 64-change
incremental limit. Source-rate and counter bounds fail closed. Feedback carries
provenance/freshness without emitting input. Publication parity tests include
C1 safe-inset, logical-target, UTF-16 text and complete decoded asset budgets.

Provider tests preserve repeated button edges, axis-specific fallback and
independent clients across reconnects, overflow and descriptor reconciliation.
They use deterministic native clocks to prove that held input expires even if
Hub I/O/UI polling is stalled while another source stays healthy. Preview and
captured-game start paths use the dedicated origin; synthetic/HID admission
cannot impersonate a companion. Existing live-output/hiding/script qualification
gates remain unchanged.

Reproduction from `cockpit`:

```text
cargo test --locked --lib --manifest-path crates/companion-service/Cargo.toml
cargo test --locked --lib --manifest-path crates/mapping-host/Cargo.toml
cargo test --locked --lib --manifest-path crates/mapping/Cargo.toml
cargo test --locked --lib --manifest-path src-tauri/Cargo.toml companion_host::tests
python scripts/companion-coverage.py --output-dir <local-output>
python scripts/mapping-host-coverage.py --output-dir <local-output>
python scripts/mapping-coverage.py --output-dir <local-output>
pnpm companion:check
pnpm companion:test
pnpm mappings:coverage
pnpm transport:test
pnpm check
pnpm ui:build
pnpm companion:ui:build
```

The native coverage command needs the pinned Rust toolchain's
`llvm-tools-preview`. `--service-only` omits Cockpit IPC when checking the shared
crate alone. Windows network-filesystem builds may use a clean local mirror and
external output directories; this does not change production sources. Native
Companion packaging uses its Tauri manifest with locked dependencies and
`custom-protocol`; this is a development build, not a signed release.

## Deferred qualification

No p95/p99 end-to-end latency, successful virtual submission or game-confirmation
measurement is claimed. Current acknowledgements say `source-accepted` and
`submitted: false`; they mean acceptance into the bounded source path. C4 owns
combined mapping/backend output preflight; C5 owns real simulator feedback.

- [#60](https://github.com/opensimgear/cockpit/issues/60): physical Windows
  second-PC LAN, firewall/interface selection, discovery/manual/QR flow, network
  interruption, OS lock/sleep/process loss, takeover isolation and recovery.
- [#58](https://github.com/opensimgear/cockpit/issues/58): packaged mixed-peer load,
  independent failure behavior and measured p95/p99 successful-submission latency.
- [#59](https://github.com/opensimgear/cockpit/issues/59): physical designer
  safe-area/touch/lifecycle checks and native file-picker round-trips.
- [#55](https://github.com/opensimgear/cockpit/issues/55),
  [#56](https://github.com/opensimgear/cockpit/issues/56): Android/iOS protected
  stores, native trust/lifecycle and signed installation.
- [#57](https://github.com/opensimgear/cockpit/issues/57): Windows desktop
  distribution/installation and broader second-PC target qualification.

Missing manual checks stay open in those stories. These implementation results
leave unqualified capabilities disabled and do not require repeated manual
controller tests for automated protocol/model changes.

The Cockpit browser development UI was also exercised against an isolated native
service: initial Disabled state and interface diagnostics, loopback-only Enable
with discovery off, C1 flight publication, bounded Make revision available upload,
and Disable all completed visibly. The isolated listener was left disabled.
These UI actions used temporary state and do not establish native-window or LAN
qualification.
