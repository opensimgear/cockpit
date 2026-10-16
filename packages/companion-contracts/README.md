# Companion contracts (C0)

Portable TypeScript contracts shared by the Svelte 5 panel renderer and future Tauri 2 Mobile / desktop companion adapters. This package performs no network, storage, pairing, HID, vJoy or mapping-host calls. Windows is the first desktop companion target; Android, iOS, Linux and macOS remain unqualified until their follow-up checks pass.

From this directory, with Node 24+ and the repository's TypeScript installed:

- npm test
- npm run coverage (minimum 71% lines, branches and functions)
- npm run check
- node examples/generate.mjs (regenerate checked JSON examples)

## Portable publications

validatePortableUi accepts schemaVersion 1 and rejects unknown keys, including credentials, executable paths, local controller assignments and target slots. Stable IDs survive label/style changes; IDs must be 1–64 ASCII letters/digits/hyphens/underscores, beginning with a letter or digit (including UUIDs). Changes to a control kind, axis range/fallback, channel binding or stable control/widget IDs are capability changes. comparePublications distinguishes these from styling/layout/label changes and exact identity.

publicationHash returns SHA-256 of canonical validated JSON (object keys sorted; array order preserved), prefixed with sha256:. Asset digests and sizes are part of that document; a transport must separately check downloaded bytes against each digest before displaying them. It does not hash absent asset bytes or establish trust. Published content is addressed by this digest and must never be mutated in place: create a new publication and digest. The C2 adapter must verify the document digest before createSessionState; the pure constructor only validates its syntax.

The initial vocabulary is buttons, normalized axes, eight-way hats and text/indicator/gauge feedback. Layout bounds are normalized to the page. Each control is placed once. The fixture panel contains two bipolar axes, two buttons, a hat and engine status. Incremental repeated button/hat transitions remain ordered; a resulting state snapshot alone cannot replace those transitions. quantizeAxis converts normalized axes into exact mapping-engine integers (bipolar −32768…32767; unipolar 0…32767), clamps finite input, applies each axis's fallback to non-finite input and rounds ties away from zero.

## Bounded protocol

All wire examples are version 1. Hello binds hostId, companionId and controllerInstanceId to a fresh clientId/sessionId; these identifiers alone do not authenticate a caller. Authentication, host certificate pinning, authorization and server-issued unpredictable session identifiers belong in the Rust C2 transport. Input, baseline, heartbeat and feedback frames bind the established session, generation and immutable publication digest.

Counters are canonical unsigned 64-bit decimal strings, never JavaScript numeric counters. A complete baseline is a separate atomic frame, sequence "0", with every control at its fallback. Subsequent input starts at sequence "1". Heartbeat and feedback sequence counters are independent of input. The receiver disarms before input/heartbeat counter rollover; an exhausted heartbeat counter or generation requires a new session.

| Budget | Bound |
| --- | --- |
| Connected companion clients / active owned targets | 8 / 8 |
| Configured logical roles, including existing physical roles | 16 |
| Controls / pages / feedback widgets per UI | 128 / 16 / 64 |
| UI document | 1 MiB |
| Assets | 2 MiB each; 8 MiB total |
| Event frame / incremental ordered changes | 16 KiB / 64 |
| Input batches | 120 per rolling second per client |
| Pending ordered transitions | 512 per client |
| Heartbeat / host lease | 250 ms / 1500 ms |
| Feedback send target | 30 Hz |
| Retained fresh session IDs per mapping-run model | 512 |

The model enforces input ordering/rate/queue/lease budgets. Input ack is a trusted host delivery acknowledgement bound to the matching session and generation, not a client message. The C2 delivery adapter must correlate and deduplicate native acknowledgements before passing counts to this model. Acks remove delivered transitions; overflow cancels the writer instead of dropping button releases. The future transport must enforce pre-authentication connection/frame limits, schedule heartbeat/tick delivery independently of UI work, throttle feedback and mark stale data; constants here do not demonstrate real network timing.

Feedback values include provenance (requested/local/host-accepted/host-submitted/game-confirmed), source kind (simulator/host/script), source age in milliseconds, a unit, and optional numeric range. They are checked against the published widget's data kind. The adapter must preserve that source age, add elapsed time since receipt and mark feedback stale after the documented deadline; receipt time does not make old simulator data fresh.

## Pure ownership and failure model

createSessionState / reduceSession model one writer per role and controller instance. connect alone never owns or arms controls. claim requires an unowned role or explicit operator-authorized takeover; a complete neutral baseline then arms the writer. Takeover emits the old writer's fallback before the new writer can arm.

Gap, malformed current-session input, rate/queue overflow, lease expiry and lifecycle cancellation emit fallback with each axis's declared value, released buttons and neutral hats. Only the affected client's generation rotates. Owned-role cleanup, role changes, takeover and explicit cancellation also invalidate a queued baseline before the writer has armed. Rearming preserves the rolling input-rate history. Rearming requires a full baseline at the fresh returned generation; stale input, baseline, cancellation and heartbeat identities cannot mutate that writer. Reconnect uses a fresh session ID; used IDs remain bounded tombstones so a retired session cannot be replayed. Reaching the 512-ID bound stops further connections until a deliberately fresh mapping-run model is created. Session IDs must remain unique across host process restarts as part of the authenticated transport design.

Host time is a trusted monotonic integer millisecond clock. Lease expiry is checked before every action and by tick; it releases ownership. A C2 scheduler must invoke tick even when no frames arrive. Pure effects are ordered instructions, not proof of native output delivery or an autonomous watchdog. Applying an effect, acknowledging native output, maintaining a worker lease, persisting credentials and reporting actual game state require later integrations.

Lifecycle reasons include blur, page hidden, touch cancellation, lost pointer capture, disconnection and layout/capability changes. Input replay and lease behavior are tested independently for multiple companions. Native mobile multitouch, background execution, touch-to-submit latency, LAN security and cross-PC delivery remain follow-up qualification.

The normative feature plan and qualification stories are in docs/companion-uis.md at the repository root. These contracts freeze the bounded C0 model; they do not enable a production companion listener.
