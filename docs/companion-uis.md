# Browser and mobile companions

Status: C0 contracts (#38), C1 designer (#39), C2 native transport/source
integration (#40), C3 mobile app/cache (#41), and C4 authored
per-instance mapping/recording/preflight (#42) are implemented for feature #37.
Physical LAN/device qualification, signed distribution and qualified game input
remain separate.
The mobile stack is **Tauri 2, Svelte 5 and TypeScript**. Desktop uses the
[Cockpit-hosted browser companion](browser-companion.md). The same panel can run
on Android, iOS or in a desktop browser; the companion is a client of Cockpit, not another
Cockpit host. All existing live-output, provisioning, hiding and script gates stay
unchanged.

The [downloadable rendering implementation plan](companion-render-packages.md)
defines proposed ESP32 content packages, widget compatibility, device budgets
and proof-of-concept acceptance for the Sunton and Waveshare boards.

## Ownership and code boundaries

This repository owns the feature. `packages/companion-contracts` owns versioned
portable documents, normalized input/feedback, validation, publication identity
and the pure session model. `apps/companion` owns a separate application identifier,
storage boundary and mobile-only Tauri shell. `apps/browser-companion` owns the
HTTPS browser client embedded in Cockpit. `packages/companion-designer` owns the schema 2
designer/library/publication model and `packages/companion-ui` supplies the shared
Svelte canvas and input semantics. `crates/companion-service` owns the narrow native
WSS transport, pairing, native protected stores and pure source-sink interface.
Cockpit integrates that interface with the dedicated `companion` mapping origin
in `crates/mapping-host`; the client crate has no controller-driver dependency.
Cockpit retains ownership of mappings, virtual devices, game profiles and feedback.

The companion must not link Cockpit's gateway/serial stack, acquire local vJoy
slots, change HidHide, launch games, expose the development bridge or accept
arbitrary native commands. It needs no controller driver on the second PC.
Cockpit persists its local designer library separately from native authority. The
companion caches up to four verified public panels (24 MiB total) with assets and page selection across restarts. Offline content remains disarmed with unavailable feedback; it grants no authority. Pairing and publishing do not start game output.

| Target | Candidate build floor | Current qualification boundary |
| --- | --- | --- |
| Android companion | Android 10 / API 29 | Candidate floor; real multitouch, lifecycle, LAN permissions and signed APK/AAB are unqualified. |
| iOS/iPadOS companion | iOS/iPadOS 15 | Candidate floor; real-device lifecycle, local-network trust and signed installation are unqualified. Requires macOS/Xcode to build. |

These floors are application decisions, not a claim that every device above them
is supported. Android Studio/SDK/NDK are required for Android builds; iOS needs a
Mac with full Xcode and Apple signing/provisioning. The CLI and API follow Cockpit's
Tauri 2 versions, and frontend dependencies share the workspace lockfile.

## Portable document and identity rules

Version 1 companions use an explicitly versioned UI document and protocol.
`uiId` (the document's `id`) names a reusable panel; `controlId` and `outputId` remain stable when moved
or restyled. A published revision has a SHA-256 content hash. Hash canonical,
validated JSON, including the declared asset digests, with recursively sorted
object keys and preserved array order. Verify each asset's exact bytes against
its own digest before loading. Do not concatenate asset bytes into the document
hash, hash arbitrary object insertion order or substitute an unverified download.
A published document is immutable.

`companionId` identifies one paired installation. `controllerInstanceId` identifies
the host's persistent logical controller/target assignment. `hostId` identifies
the pinned host, independently of IP/DNS address. Each authenticated connection
has a fresh `sessionId` and generation. IDs are routing keys, not credentials.
`clientId` is the authenticated host's connection routing slot, bound to the
installation and controller instance during admission. Input frames refer to
that binding; a caller-chosen ID cannot grant authority by itself.
Reinstallation or credential loss requires re-pairing; a reused display name or
IP address never recovers authority. Counters are canonical decimal uint64 strings
so JavaScript cannot silently lose precision.

Portable exports contain UI definitions, capabilities and content-addressed asset
references only. They reject pairing secrets, tokens, host trust records, machine
paths, credentials and local virtual-target assignments. Host assignments and app
credentials are separate local stores. Compatibility-changing edits require a
new descriptor generation and explicit reconciliation; styling-only revisions
preserve logical bindings but still publish a new immutable content revision.
Older documents are read through explicit migration or rejected with a version
diagnostic; never reinterpret unknown fields or silently truncate controls.

## Initial budgets

| Resource | Version 1 limit / target |
| --- | --- |
| Active companion controllers | 8 maximum, subject to the shared mapping budgets below |
| One UI | 16 pages, 128 logical inputs, 64 feedback widgets |
| Serialized UI | 1 MiB maximum UTF-8 |
| Assets | 2 MiB each, 8 MiB total; content hashes checked before use |
| Input frame | 16 KiB UTF-8, at most 64 changes |
| Input rate | 120 batches/second/client; button edges remain ordered |
| Ordered transition queue | 512 entries/client; overflow disarms/resyncs that instance |
| Feedback | At most 30 updates/second/client, coalesced typed state |
| Heartbeat / host lease | Every 250 ms / expires after 1500 ms without authenticated liveness |
| Touch-to-successful-submit release target | p95 at most 25 ms, p99 at most 50 ms on the qualification LAN |
| Feedback freshness target | At most 250 ms; mark stale after 1000 ms without fresh source data |

Latency/freshness numbers are release targets, not current measurements. Report
client render time, network time, host processing, successful virtual submission,
queues, dropped/coalesced data and faults separately. Configured gesture delays
and game sampling are separate. C6 must qualify the chosen network/device/load
matrix. C4 coordinates the ordinary physical/panel mapping and bounded companion
routes in one workspace. Each companion route has its own execution worker and
Stop scope, while all routes share 16 total source roles, eight total targets and
256 total nodes. This preserves the C0 resource envelope across execution engines.
Eight companions is a ceiling, not eight guaranteed free slots; preflight reserves
every role, target and node before arming and rejects unsupported backend layouts.

## Sessions, ordering and isolated failure

The C0 reducer is an executable contract, not a network authentication service.
C2's native Hub implements authentication, instance authorization and session
ordering before its `SourceSink` callbacks can update mapping input. One controller instance has one writer. A second writer is
rejected; explicit takeover first releases the old instance and increments its
generation. Stop/revoke/failure affects that instance, not global `mapping_stop`.
Other companions, physical inputs, gateways and integrations remain running.

Input starts disarmed. A complete neutral baseline uses sequence `0`, then input
batches are strictly contiguous. Momentary controls already held when connecting
must be released before rearming. A gap, wrong revision, obsolete session,
overflow, lease expiry or lifecycle loss cannot replay earlier input. Establish a
new generation/baseline before accepting new input. Heartbeats have a separate
counter: a held button or stationary axis is healthy while authenticated
liveness continues; heartbeats do not invent input transitions.

The complete baseline is a separate frame containing all controls (up to 128),
not an incremental input batch subject to the 64-change limit. Wire axis values
are finite normalized numbers: bipolar [-1,1], unipolar [0,1]. C4 quantizes bipolar
negative values by 32768, nonnegative values by 32767, and unipolar values by
32767; round to the nearest integer with ties away from zero. Thus -1,0,1 map
exactly to -32768,0,32767. Validate fallback in the same range before conversion.
Hats use neutral -1 and directions 0–7; buttons are booleans. Backend capacity
validation remains separate from portable document validation.

Cancellation releases buttons/hats and applies each axis's declared fallback;
zero is not universal neutral. Cancel owned repeats, pulses, macros, timers and
pending transitions. Reconnection, app relaunch and host restart are distinct
events; saved configuration survives, active input does not silently resume.
Desktop blur disarms by default. Page/layout change, pointer cancellation/capture
loss, close, hide, lock/sleep, backgrounding and process death all require local
cleanup where possible and host-lease fallback when a final frame cannot arrive.
No framework can guarantee a final send after an OS kill.

Tauri Rust mobile suspend/resume handling supplements DOM pointer/visibility/focus
events. Do not infer iOS coverage from an Android-only JavaScript lifecycle API.
Native OS lifecycle and touch qualification remain separate from injected events
and a local renderer preview.

## LAN protocol and trust bootstrap

C2 supplies **one native Rust WSS listener** for all companion UIs/instances, not
the broad development bridge. Each native mobile companion uses an app-owned Rust WSS client
with explicit trust verification. Browser trust and one-use URL pairing follow
the [browser guide](browser-companion.md). Stock WebSocket plugin availability alone does
not establish pairing-bound/self-signed LAN certificate support. All targets use
the same versioned envelopes, authorization and limits. C2's UI default is
TCP 54437 (user-configurable); a bind conflict is diagnosed without silently
switching an already paired endpoint. Discovery advertises the actual address/port
under `_osg-companion._tcp.local.` and is an untrusted connection hint;
manual host/port and QR remain available. Listener enablement and interface binding
are explicit; do not add a firewall exception or expose all interfaces silently.

The host generates a persistent private CA in Windows protected storage and a
server leaf certificate signed by that CA. The CA is self-signed; the server leaf
is not substituted as a root. A QR pairing offer contains protocol version,
`hostId`, endpoint, the CA certificate and its SHA-256 public-key/SPKI pin, expiry
and a cryptographically random
256-bit one-use secret. It expires after 60 seconds. Before transmitting that
secret, the companion verifies the TLS peer against the offered pin, certificate
validity and host identity. Connect-address changes do not grant new authority.
Manual pairing requires a full SPKI fingerprint copied/compared through an
authenticated out-of-band path with the host screen; it never silently trusts
the first LAN certificate. A convenient short-code alternative needs a separately
reviewed PAKE/SAS protocol with transcript binding, not a truncated fingerprint.

The current native client verifies the actual advertised endpoint host/IP against
the signed leaf's SAN using ordinary rustls verification. `hostId` is checked again
in the authenticated session response. The offered CA pin is the only trust
anchor; chain signatures, server use, validity and SAN checks remain enabled.
No public-CA trust or machine-wide trust installation is needed. Enabling a new
advertised address renews the leaf under the retained CA. A saved client endpoint
is not silently rewritten from discovery; changing it requires an explicit new
trusted pairing payload. CA-key rotation requires re-pairing. Pin mismatch, expired
certificate, clock skew or revoked credentials disarm with an actionable error.
Never disable TLS verification globally, install a machine-wide trust exception
or send credentials over plaintext as a fallback.

The listener limits unauthenticated attempts (5/minute per address, 20/minute
globally), bounds handshakes and pairing offers, and requires host approval of
the requesting installation/permissions. A successful exchange consumes the
one-use secret and issues a revocable per-installation credential. Cockpit persists host authority and trust/private keys in protected native storage. Mobile credentials use Apple Keychain or the Android Keystore bridge. Missing/locked stores fail closed. Mobile authentication remains disabled pending #55/#56 before secret exchange. Do not put secrets in UI
exports, logs, URLs or web localStorage.
Revocation terminates that installation's sessions and releases its controls.

Session establishment negotiates protocol/schema, authorized UI revision,
controller instance and budgets. Messages distinguish authentication, baseline,
input, heartbeat, acknowledgement, resync, feedback and close. An acknowledgement
must state whether it means host receipt or successful virtual submission; neither
is game confirmation. Typed feedback carries provenance (requested/local,
host-accepted/submitted or game-confirmed), freshness and units/range. Unknown or
stale data stays visible; feedback never emits controller input.

Download assets by authorized revision/hash with byte limits before allocation or
decompression. Refuse traversal, arbitrary external URLs, executable content and
hash mismatches. A slow or malformed client must not block another client's
queue, gateway worker, mapping clock or feedback. C2 implements these boundaries and exercises real WSS peers on local Windows
sockets. Second-PC LAN and mobile native trust/storage qualification remain open;
local socket tests are not physical-network release evidence.

## Authored companion mappings (C4)

In Cockpit's Mappings page, use **Companion controller mappings** to select a
persistent instance, inspect its trusted publication and save its authored route.
The native host derives source roles, target indices, axis ranges, declared
fallbacks and bindings from that immutable publication. Two copies of the same UI
receive distinct instance, profile and target identities. The manual mapping
editor remains available for existing physical/panel workflows.

**Save for recording only** needs no virtual driver. **Start selected recording**
requires current assignment authority and waits for a complete source baseline
before recording input reports through the production mapping engine. The report log identifies
the target, sequence, state and reason; these are software reports, not evidence
that a game received input. Saving, pairing, reconnecting or restoring settings
never starts a route.

For live output, inspect native targets and explicitly save exact output
assignments, including native axis usages. Preflight checks the pinned revision,
authorization, baseline, explicit output enablement, backend qualification,
capabilities, inventory, recovery and shared reservations. Missing or extra axes,
buttons or hats, duplicate slots, mismatched fallback and over-budget layouts are
rejected rather than truncated. Publishing a changed revision requires Stop and
explicit reconciliation; a new baseline cannot revive an old route.

Each route has a bounded worker and separate recovery journal. A slow feeder does
not run on the ordinary mapping worker or another companion's worker. Stop
invalidates queued/in-flight Start for that instance, cancels pending actions and
applies its declared fallback. Pending or failed cleanup remains visible and
retains resource ownership until recovery completes. Source loss, revocation,
lease expiry and restart require a fresh baseline and explicit Start. Existing
physical mappings and other companion instances retain their own sessions.

Browser and mobile companion clients share the host authorization model;
the client OS does not select or qualify the host output backend. Recording is
available while live output is unqualified. Linux/macOS host output and multiple
live feeder processes on those hosts remain gated by their backend limitations
and #48/#49. Current production qualification gates remain disabled, including
Windows output pending #54. No test-only feeder can be selected from the UI.

Real-game routing, game binding retention, physical fallback and browser/mobile
OS combinations remain in [#66](https://github.com/opensimgear/cockpit/issues/66).
Native installation, protected-store and LAN/lifecycle checks remain in the
platform stories below. Automated local WSS/recording and injected feeder tests
do not replace those checks.
## Delivery, schema migration and validation

C1 delivers the designer and full shared renderer. C2 adds the native listener,
pairing, Rust client/session boundary and a distinct `companion` mapping source
origin through an explicit schema migration. Existing `host-controller`, panel
and synthetic sources are not reused to impersonate a remote controller. Preserve
existing imports and recording behavior; production startup never enables new
listener/input effects implicitly. C3 delivers the Android and iOS native shells. C4 coordinates isolated companion engines with the ordinary mapping,
reserves shared resources and checks exact backend layouts before acquisition.
C5 adds feedback and captured game settings. C6 qualifies distribution/lifecycle,
mixed peers, performance and supported host/backend combinations.

CI checks contracts and lifecycle tests with at least 71% measurable lines,
branches and functions, TypeScript/Svelte checks, the renderer build, and an Android
native build job. Mobile signing and real devices are separate
checks. C0's local recording prototype sends no virtual reports, opens no LAN
listener and cannot pair with a host. Do not describe running it on one PC as
second-PC networking or Android/iOS proof. Follow-up qualification stories retain
missing checks rather than changing unchecked criteria into passes.

The C0 reference renderer has a fixed panel using normalized buttons, axes and a
hat. It is not an arbitrary-document designer/renderer. C1 supplies the complete
presentation/preset vocabulary, grouping, selectors, encoders and portable asset
workflow; any incompatible document extension gets an explicit version/migration.
Normalized primitive controls do not restrict later widgets to the reference
panel's appearance.

| Missing native/release check | Follow-up |
| --- | --- |
| Android device lifecycle, trust and signed install | [#55](https://github.com/opensimgear/cockpit/issues/55) |
| iOS/iPadOS device lifecycle, trust and signed install | [#56](https://github.com/opensimgear/cockpit/issues/56) |
| C4 real-game routing, binding retention, fallback and client OS matrix | [#66](https://github.com/opensimgear/cockpit/issues/66) |
| Packaged mixed-peer trust, load and measured end-to-end performance | [#58](https://github.com/opensimgear/cockpit/issues/58) |

Primary framework references: [Tauri mobile plugins](https://v2.tauri.app/develop/plugins/develop-mobile/),
[mobile prerequisites](https://v2.tauri.app/start/prerequisites/),
[WebSocket API](https://v2.tauri.app/reference/javascript/websocket/),
[Rust mobile lifecycle](https://docs.rs/tauri/latest/x86_64-apple-ios/tauri/enum.WindowEvent.html),
and [Apple local-network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy).

## C1 designer and shared renderer checkpoint

C1 ([#39](https://github.com/opensimgear/cockpit/issues/39)) supplies the visual
designer and arbitrary-document canvas used by Cockpit and the separate
Companion app. `packages/companion-designer` owns schema 2 documents, editing,
history, portable target bindings, asset bundles and immutable publication;
`packages/companion-ui` owns the shared Svelte rendering and recording-only input
semantics. Schema 2 explicitly wraps the validated C0 logical schema and migrates
legacy panels while preserving their logical IDs.

The outer publication hash covers the full validated designer document, including
widget behavior and asset digests. Semantic changes such as toggling, pulse/repeat,
selector values, axis policies and target bindings change its capability
fingerprint. Styling edits retain logical bindings. Assets are verified against
their own bytes before use. Native sessions must bind the outer publication hash;
hashing only the embedded C0 logical UI would omit renderer behavior.

The local designer supports saved panels, pages, movement/resizing, grid,
alignment, groups, undo/redo, validated properties, templates, static raster
assets, logical controller assignment, import/export, target-size preview and
explicit publication. It never allocates a driver slot or starts game input.
Drafts and immutable published snapshots are persisted atomically, with hash and
asset validation on load. Local storage is bounded to 20 drafts, 100 published
revisions and 64 MiB; portable documents/bundles retain their separate limits.

Local tests, typechecks, frontend builds and browser interaction are recorded in
[the C1 verification record](verification/2026-10-16-companion-c1.md). Two-panel
document/storage/asset bundle round-trips passed automatically. Browser checks
covered two templates, save/reopen, page/widget editing and phone portrait preview.
The native file-picker attempt stalled and remains unverified. C1 qualification
follow-up [#59](https://github.com/opensimgear/cockpit/issues/59) retains native
file-picker round-trips and real phone/tablet/second-PC safe-area, touch and
lifecycle checks. Existing Android/iOS,
networking, signing and mixed-peer release qualifications remain open as well.

C2 supplies native host networking, pairing and a dedicated companion
source, described below. C4 still owns combined virtual-output preflight and
submission qualification. Existing unqualified capability gates remain disabled.


## Host transport and mobile client

Cockpit runs the shared HTTPS/WSS listener. Desktop users follow [browser setup and pairing](browser-companion.md). The native mobile shell retains the pinned Rust WSS client, manual pairing and explicit Connect/Start workflow, with authentication gated pending #55/#56. See [mobile instructions](../apps/companion/README.md).

The host UI exposes instance Disconnect and explicit Takeover, plus installation
Revocation and listener Disable. A remote client cannot take control merely by
setting `takeover: true` on `open`; host-side authorization releases the old writer
first. The replacement must open a fresh session and establish its own baseline.

Cockpit retains paired authority and listener preferences across restarts. Active sessions and held input do not resume. Native mobile clients restore saved trust disarmed and require explicit Connect and Start.

## Implemented C2 wire and source boundaries

The native mobile route is `wss://host:port/companion` with subprotocol
`opensimgear-companion-v1`. This route rejects query strings, browser `Origin`
headers, binary commands and the general Cockpit gateway protocol. The browser
uses a separate same-origin route with exact Origin admission. The native
client accepts LAN/loopback destinations and performs the offered CA-pin check
before DNS/socket access or sending the pairing secret. Each peer has its own
bounded socket worker. Admission allows 16 socket workers, eight authenticated
connections globally and two per paired installation. Pre-auth attempts are
limited to five/minute/IP and 20/minute globally, with a three-second absolute
TLS/HTTP handshake deadline and 32 KiB HTTP-upgrade budget.

The implemented bootstrap precedes the C0 identity-framed messages:

| Step | Request / result |
| --- | --- |
| New pairing | `pair` with one-use secret, installation ID and name; `pair-pending` returns a request ID. `pair-status` returns a credential only after local host approval. |
| Authentication | `auth` with the protected installation credential; `authenticated` returns the authorized assignment list. |
| Open | `open` with an authorized `instanceId`; `session-open` returns host/installation/controller identity, a fresh client/session/generation binding and the exact immutable publication. |
| Baseline | C0-style identity envelope plus `type: baseline`, sequence `"0"` and all normalized control fallback values. This is atomic and supports all 128 controls. |
| Ordered input | The same envelope plus `type: input`, contiguous decimal sequence and at most 64 ordered changes. Repeated press/release transitions are preserved. |
| Liveness | `heartbeat` has its own increasing decimal sequence, independent of input. Stationary/held input stays valid while authenticated liveness continues. |
| Result | `ack` states `ackStage: source-accepted`, `submitted: false`; it may include typed feedback and freshness. This acknowledges bounded source acceptance, not virtual submission or game confirmation. |
| Cancellation | Identity-bound `close`/`cancel`, socket loss, revoke, takeover, malformed current input, gaps, overflow and lease expiry release the affected source. |

Identity-framed messages carry `version`, `clientId`, `sessionId`, `generation`
and `publicationHash`; the host binds these to the authenticated connection,
installation and controller instance. C0's `Hello` fixtures remain the pure model's
admission examples, not an additional network command. All counters are decimal
uint64 strings. Old identities are rejected before they can mutate a replacement
writer. Any rearm after invalidation requires a fresh generation/baseline or an
explicit new connection/session; counter exhaustion fails closed.

A publication reply larger than 16 KiB uses ordered `transfer-start`,
`transfer-chunk` and `transfer-end` envelopes, bounded to 13 MiB with a transfer
SHA-256. The native client checks the outer schema 2 document, canonical JSON,
content/capability hashes and exact asset bytes. The Svelte client also validates
and decodes the shared document/assets before allowing Start. Assets are static
PNG/JPEG/WebP, at most 2 MiB each/8 MiB total; dimensions are at most 8192 per side
and 16,777,216 pixels. Publication verification never grants input authority.

The provider registers persistent logical instance IDs under `Origin::Companion`.
It never calls a companion Synthetic, physical HID or gateway source. Mapping
schema 1 imports retain their interpretation; companion roles require explicit
schema 2. UI option keys may use `companion:<id>` only to avoid collisions; saved
native assignments contain the bare instance ID and the explicit origin.

The mapping worker drains isolated per-instance queues using the native host
clock, independently of UI polling. Each source has a 512-transition queue;
overflow clears that source and requires a fresh baseline. Fair bounded draining
keeps a busy peer from starving another. Wire baseline sequence zero becomes a
positive native baseline sequence, with separate native counters so filtered
unassigned controls do not create artificial gaps. The provider renews its own
1500 ms native-clock lease on accepted baseline/input/heartbeat and checks expiry
on mapping drain, even if Hub persistence or network workers are stalled.

The client serializes ordered IPC batches, rejects stale native epochs, bounds
backlog and stops expired queued input rather than replaying it. DOM/native
lifecycle cancellation and a separate UI keepalive supplement the host lease.
These are implemented failure controls; they do not prove every OS sleep/lock or
mobile lifecycle path. Typed feedback retains provenance, units/range and source
age, and cannot emit input. C5 adds the native simulator-feedback integration described below.

Local evidence is recorded in [the C2 verification record](verification/2026-10-16-companion-c2.md).
[#58](https://github.com/opensimgear/cockpit/issues/58) retains packaged mixed-peer
load and measured p95/p99 successful-submission latency. Neither local WSS tests
nor source-accepted acknowledgements establish those performance targets.

## C3 mobile application

The native mobile app retains persistent verified offline panels, explicit recovery and guarded async session/view state. Android/iOS authentication remains disabled pending native qualification. See [mobile build and storage instructions](../apps/companion/README.md). Desktop uses the [browser companion](browser-companion.md).

## C4 isolated controller mappings

Each retained companion instance has its own immutable controller revision,
recording/output profile, worker, reservation and recovery boundary. Start is an
explicit operation, with the current native backend qualification and enablement
checks. See [C4 validation](verification/2026-10-16-companion-c4.md).

## C5 typed feedback and captured game assignments

Browser and Android/iOS companions share the Svelte renderer and typed
feedback model. The host's Companion connections page exposes
**Feedback** for a selected instance. Save explicit bindings from a publication's
feedback widget/channel to requested state, host acceptance, acknowledged native
submission or an integration source. Numeric sources require compatible units and
ranges; an explicit scale/offset converts values before the bounded destination
range is checked. Text and boolean routes retain their types.

The renderer labels provenance independently from freshness. Requested, Local,
Host accepted, Host submitted and Game confirmed are separate states; missing,
stale or disconnected data cannot retain a fresh confirmation. Host submitted
requires an actual native feeder acknowledgement. Recording and source acceptance
cannot establish submission. ACR's synthetic source is Local; DCS-BIOS or the
supported simulator provider can establish Game confirmed only from its fresh
native decoder. This does not add a Star Citizen telemetry feed.

Integration documents keep schema 1 behavior. Schema 2 may add up to 16 combined
physical/feedback outputs, with source-only `feedbackOutputs` and no module roles.
These decode supported telemetry without acquiring a panel or inventing physical
output targets. Bindings pin the semantic integration configuration, channel
metadata and immutable publication. A changed source requires an explicit save.
Transient game process IDs do not invalidate an unchanged source definition.

Visual Reset clears displayed values to unknown until the source changes;
Resync rotates the feedback generation and obtains a native snapshot. Neither
operation sends control input, starts mappings or creates recording reports.
Session, publication, generation, controller and output identities are checked on
both sides. Ordered chunk transport supports bounded full snapshots, with one
socket worker per peer. Slow or disconnected peers cannot run network I/O on the
mapping, gateway or integration worker.

Game profiles may optionally capture the current companion assignment identities
and immutable publication/capability hashes. Schema 3 adds that capture to game
profiles and recovery journals; schema 1/2 continue to load. Import, capture and
save do not connect or arm a controller. Activation preflights all captured
requirements before effects, stops the previous integration before taking the
assignments and records their baseline. Stop/recovery restores only the exact
state owned by that game session. External edits cause a visible restoration
conflict; they are not overwritten. Active game ownership guards assignment and
feedback-route edits, and preserves emergency Stop/disconnect/revoke operations.

[C5 verification](verification/2026-10-16-companion-c5.md) distinguishes local
software evidence from [native qualification #67](https://github.com/opensimgear/cockpit/issues/67).
Mobile authentication, virtual output, provisioning, hiding and script capability
gates retain their existing disabled/unqualified state. Native companion client
support does not qualify Linux/macOS host output backends (#48/#49).
## C6 release preparation

The [user guide](companion-guide.md) covers designer/import, pairing, independent
recording routes, feedback, captured game assignments, readiness and recovery.
[Flight/button-box and axis/racing examples](../examples/companion/README.md) are
portable designer imports; they contain no credentials or local assignments and
do not connect or activate output when imported.

The [release matrix](companion-release-matrix.md) lists browser and Android/iOS clients, exact limits and pending qualification. No client target is release-qualified. Signed mobile updates remain #68; device/LAN/lifecycle, real game/feedback and measured load remain #55/#56/#59/#66/#67/#58. Feature #37 remains open.
