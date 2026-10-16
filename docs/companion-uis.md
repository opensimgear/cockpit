# Mobile and desktop companion contracts (C0)

Status: implementation/feasibility work for #37/#38. This is not a production LAN
listener, a full designer, signed companion distribution or qualified game input.
The selected stack is **Tauri 2, Svelte 5 and TypeScript**. The same panel can run
on Android, iOS or a separate PC; the companion is a client of Cockpit, not another
Cockpit host. All existing live-output, provisioning, hiding and script gates stay
unchanged.

## Ownership and code boundaries

This repository owns the feature. `packages/companion-contracts` owns versioned
portable documents, normalized input/feedback, validation, publication identity
and the pure session model. `apps/companion` owns a separate application identifier,
storage boundary, Svelte renderer and minimal Tauri shell. The renderer is kept
separate from shell APIs so C1's designer preview can reuse it. Cockpit's existing
root app and Rust crates continue to own designer integration, listener, source
registration, mappings, virtual devices, game profiles and simulator feedback.

The companion must not link Cockpit's gateway/serial stack, acquire local vJoy
slots, change HidHide, launch games, expose the development bridge or accept
arbitrary native commands. It needs no controller driver on the second PC.
Published assets are bundled/cached locally; an offline panel remains viewable
with input disarmed. Pairing and publishing do not start a controller.

| Target | Candidate build floor | C0 qualification boundary |
| --- | --- | --- |
| Windows desktop companion | Windows 10/11 x64, WebView2 | Native local build/recording prototype; second-PC networking, sleep/lock and signed installation require qualification. |
| Android companion | Android 10 / API 29 | Candidate floor; real multitouch, lifecycle, LAN permissions and signed APK/AAB are unqualified. |
| iOS/iPadOS companion | iOS/iPadOS 15 | Candidate floor; real-device lifecycle, local-network trust and signed installation are unqualified. Requires macOS/Xcode to build. |
| Linux/macOS desktop companions | Deferred; no minimum selected | Shared design is portable; no desktop package or permission/signing support claim. These do not block Windows. |

These floors are application decisions, not a claim that every device above them
is supported. Android Studio/SDK/NDK are required for Android builds; iOS needs a
Mac with full Xcode and Apple signing/provisioning. Windows desktop development
uses the existing Rust/MSVC/WebView2 toolchain. The CLI and API follow Cockpit's
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
matrix. Current mapping limits are one active run, 16 total source roles, eight
total targets and 256 total nodes. Physical/panel sources share those budgets.
Eight companions is a ceiling, not eight guaranteed free slots; preflight reserves
every role, target and node before arming and rejects unsupported backend layouts.

## Sessions, ordering and isolated failure

The C0 reducer is an executable contract, not a network authentication service.
The future listener authenticates and authorizes a frame before passing it to
the session model. One controller instance has one writer. A second writer is
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
and a Windows local preview.

## LAN protocol and trust bootstrap

C2 supplies **one native Rust WSS listener** for all companion UIs/instances, not
the broad development bridge. Each companion uses an app-owned Rust WSS client
with explicit trust verification. Stock WebSocket plugin availability alone does
not establish pairing-bound/self-signed LAN certificate support. All targets use
the same versioned envelopes, authorization and limits. C2's proposed default is
TCP 54437 (user-configurable); a bind conflict is diagnosed without silently
switching an already paired endpoint. Discovery advertises the actual address/port
under `_osg-companion._tcp.local.` and is an untrusted connection hint;
manual host/port and QR remain available. Listener enablement and interface binding
are explicit; do not add a firewall exception or expose all interfaces silently.

The host generates a persistent key and self-signed certificate in local native
storage. A QR pairing offer contains protocol version, `hostId`, endpoint,
SHA-256 certificate public-key/SPKI pin, expiry and a cryptographically random
256-bit one-use secret. It expires after 60 seconds. Before transmitting that
secret, the companion verifies the TLS peer against the offered pin, certificate
validity and host identity. Connect-address changes do not grant new authority.
Manual pairing requires a full SPKI fingerprint copied/compared through an
authenticated out-of-band path with the host screen; it never silently trusts
the first LAN certificate. A convenient short-code alternative needs a separately
reviewed PAKE/SAS protocol with transcript binding, not a truncated fingerprint.

Use a stable synthetic DNS name derived from `hostId` in the certificate SAN and
TLS server-name verification. A native connector reaches the discovered/manual
IP but verifies that stable server name; DNS resolution of the synthetic name is
not required. Treat the offered pin as the trust anchor for the self-signed host,
retaining certificate signature, key-usage, validity and SAN checks; do not require
public-CA trust as well. Endpoint IP/discovery may change independently. Certificate/public-key rotation
requires an explicit trusted transition or re-pairing. Pin mismatch, expired
certificate, clock skew or revoked credentials disarm with an actionable error.
Never disable TLS verification globally, install a machine-wide trust exception
or send credentials over plaintext as a fallback.

The listener limits unauthenticated attempts (5/minute per address, 20/minute
globally), bounds handshakes and pairing offers, and requires host approval of
the requesting installation/permissions. A successful exchange consumes the
one-use secret and issues a revocable per-installation credential. Persist it in
native secure storage (Android Keystore-backed, Apple Keychain, Windows protected
storage); do not put secrets in UI exports, logs, URLs or web localStorage.
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
queue, gateway worker, mapping clock or feedback. TLS/session/authentication and
OS secure-storage implementations are C2/C3 work; C0 freezes these requirements
and tests portable semantics, without claiming a qualified production transport.

## Delivery, schema migration and validation

C1 delivers the designer and full shared renderer. C2 adds the native listener,
pairing, Rust client/session boundary and a distinct `companion` mapping source
origin through an explicit schema migration. Existing `host-controller`, panel
and synthetic sources are not reused to impersonate a remote controller. Preserve
existing imports and recording behavior; production startup never enables new
listener/input effects implicitly. C3 delivers Android, iOS and desktop companion
packages. C4 composes isolated companion sources within one coordinated mapping
run and checks backend layouts; it does not create unrelated concurrent runs.
C5 adds feedback and captured game settings. C6 qualifies distribution/lifecycle,
mixed peers, performance and supported host/backend combinations.

CI checks contracts and lifecycle tests with at least 71% measurable lines,
branches and functions, TypeScript/Svelte checks, the renderer build, and a Windows
native shell compilation/test job. Mobile signing and real devices are separate
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
| Second Windows PC, lock/sleep/network and installation | [#57](https://github.com/opensimgear/cockpit/issues/57) |
| Packaged mixed-peer trust, load and measured end-to-end performance | [#58](https://github.com/opensimgear/cockpit/issues/58) |

Primary framework references: [Tauri mobile plugins](https://v2.tauri.app/develop/plugins/develop-mobile/),
[mobile prerequisites](https://v2.tauri.app/start/prerequisites/),
[WebSocket API](https://v2.tauri.app/reference/javascript/websocket/),
[Rust mobile lifecycle](https://docs.rs/tauri/latest/x86_64-apple-ios/tauri/enum.WindowEvent.html),
and [Apple local-network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy).
