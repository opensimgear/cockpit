# Native companion service

`osg-companion-service` implements Windows C2 pairing, trust, transport and
per-instance input sessions for Cockpit and the separate Tauri Companion app.
It has no mapping, HID, vJoy, serial, game-launch or gateway dependency. The host
adapts its `SourceSink` interface to a dedicated mapping source; clients therefore
do not link controller drivers.

## Components

| Module | Responsibility |
| --- | --- |
| `Hub` | Local operator authority, installation pairing/revocation, persistent assignments, authenticated instance/session admission, ordered baseline/input/heartbeat and isolated cancellation |
| `publication` | Strict C1 outer schema 2/C0 logical schema validation, canonical JSON/content and capability hashes, bounded static raster assets and portable bindings |
| `sources` | Logical descriptors, normalized values, exact integer axis quantization and the nonblocking in-memory `SourceSink` boundary |
| `server` | One opt-in narrow WSS listener, independent bounded workers, handshake/admission limits, discovery hints and chunked verified publication transfer |
| `tls` | Persistent pinned host CA, signed server leaf and ordinary rustls signature/validity/SAN checks |
| `persistence` | Bounded atomic per-user Windows DPAPI state; other OSes fail closed without plaintext fallback |
| `client` | Protected credential/trust, pairing approval flow, publication verification, native epochs, ordered bounded input and lifecycle/UI/host lease controls |

## Host integration

Create `Hub::open(application_directory, Arc<dyn SourceSink>)`, then create
`ServerRuntime::new(hub.clone(), application_directory)`. Constructing either
restores local identity/authority but does not open a listener or arm input.
Explicitly call `start(ServerConfig)` after the operator chooses the interface,
advertised reachable address and port; call `stop()` during disable/shutdown.
Cockpit's default UI port is 54437. No port fallback or firewall modification is
performed. Discovery (`_osg-companion._tcp.local.`) is optional and never conveys
trust.

`Hub::command` is a **local operator API**, not a WSS command router. It publishes
validated revisions and manages pairing offers/approval, assignments, disconnect,
takeover, revocation and typed feedback. The WSS endpoint exposes only the bounded
companion bootstrap/session protocol. Do not route untrusted peer messages into
operator commands.

Implement `SourceSink::register`, `baseline`, `changes`, `heartbeat` and `loss` as
bounded in-memory updates. Tokens carry persistent source/instance ID plus fresh
session, generation and publication hash. `baseline` uses wire sequence zero and
all fallbacks; incremental changes are ordered and may contain repeated digital
transitions. Do not coalesce a press/release pair. `loss` affects only that source,
releases buttons/hats, restores its declared axis fallback and cancels owned
pending work. Callbacks must not wait on network I/O or a global mapping-run lock.
Cockpit's provider also checks an independent native-clock lease while draining,
so a stalled Hub persistence call cannot leave held input active.

Assignments and credential hashes are restored offline from protected host state.
Publication documents/assets are deliberately an in-memory host cache (32
revisions/64 MiB); after restart, explicitly republish required saved revisions
before opening sessions. Persisting the designer's library is the application's
separate responsibility. Active sessions and listener enablement never persist.

## Client integration and trust

Create `ClientRuntime::open(application_directory)`, then use its local command
API: `pair-start`, `refresh-assignments`, `connect`, `arm`, `input`, `keepalive`,
`neutralize`, `disconnect`, `forget`, `status`, `publication` and `discover`.
`connect` verifies the publication and sends the complete neutral baseline; only
its acknowledgement permits the local `arm` action. Input/keepalive commands carry
the current native epoch. Reconnect and Start remain explicit after any lifecycle
loss or invalidation. Keep the UI keepalive independent of pointer activity so a
healthy held control is not mistaken for a dead renderer. A stalled UI stops the
native session rather than renewing a remote held input forever.

A pairing offer contains version, endpoint, host ID, the CA certificate/SPKI pin,
a 256-bit one-use secret and a 60-second expiry. Receive it through an authenticated
out-of-band path. The native connector validates the pin before socket access or
secret transmission. The private CA is self-signed; its server leaf is separately
signed, constrained for server use, checked for the advertised host/IP SAN and
validated by rustls. The leaf is not accepted as a CA. Public-CA trust and global
certificate-verifier overrides are unnecessary. CA changes require re-pairing;
discovery does not rewrite saved trust or endpoints.

Only Windows DPAPI-backed credentials/private keys are implemented. Android/iOS
pairing and connect stop before secret exchange until their protected-store
adapters are qualified. Browser rendering remains a local preview. The native
client stores credential/trust and assignments, not a durable downloaded-panel
asset cache. Never put credentials in UI exports, logs, URLs or web storage.

## Wire summary and bounds

The endpoint is `wss://host:port/companion` with subprotocol
`opensimgear-companion-v1`; no query, browser Origin or arbitrary binary/native
commands are accepted. Bootstrap uses `pair`/`pair-status`, then `auth` and `open`.
The returned `session-open` binds host, installation, controller instance,
client/session/generation and publication. Subsequent version 1 frames repeat
`clientId`, `sessionId`, `generation` and `publicationHash` with `baseline`,
`input`, independent `heartbeat`, or `close`/`cancel`. The C0 `Hello` fixture is a
pure-model admission example, not another WSS bootstrap operation.

- Eight authenticated connections globally, at most two per installation; 16
  socket workers. Five pairing/auth attempts/minute/IP and 20/minute globally.
- Three-second absolute TLS/HTTP handshake deadline, 32 KiB HTTP-upgrade budget.
- 16 KiB JSON frames; at most 64 ordered incremental changes. The separate atomic
  baseline supports all 128 controls. Counters are canonical decimal uint64 strings.
- 120 input batches/second/session, 250 ms heartbeat and 1500 ms host lease.
  Mapping-provider queues independently cap 512 transitions/instance. The native
  client also bounds IPC/backlog and stops expired queued input.
- One document at most 1 MiB; 16 pages, 128 controls and 64 feedback widgets.
  Static PNG/JPEG/WebP assets: 2 MiB each/8 MiB total, 8192 per side and 16,777,216
  pixels. Transfer-start/chunk/end is bounded to 13 MiB and SHA-256 verified.
- Feedback is typed and bounded to 30 updates/second, with provenance, source age,
  unit and optional range. It never emits controller input.

An acknowledgement states `ackStage: source-accepted`, `submitted: false`.
Acceptance into a source queue is not virtual-device submission or game
confirmation. Gaps, obsolete identities, malformed current input, rate/queue
exhaustion, lease loss and explicit cancellation fail closed for the affected
instance. A second remote writer cannot bypass host-side takeover authorization.

## Tests and qualification

From `cockpit`:

```text
cargo test --locked --lib --manifest-path crates/companion-service/Cargo.toml
python scripts/companion-coverage.py --service-only --output-dir <local-output>
```

The Windows coverage script requires `llvm-tools-preview`; omit `--service-only`
to include Cockpit operator IPC coverage. Every measured production module has a
71% line floor. Real local WSS tests exercise three clients, independent source
loss, trust rejection, publication chunks, oversized frames, stalled handshakes,
port conflicts and protected restart. These tests do not qualify a physical LAN,
mobile lifecycle or signed installation.

See [the architecture and usage guide](../../docs/companion-uis.md),
[the C2 verification record](../../docs/verification/2026-10-16-companion-c2.md),
[Windows LAN qualification #60](https://github.com/opensimgear/cockpit/issues/60)
and [mixed-peer/performance qualification #58](https://github.com/opensimgear/cockpit/issues/58).
Existing unqualified game-output, hiding and script capability gates remain
unchanged. Source-accepted timing is not the p95/p99 successful-submission target.
