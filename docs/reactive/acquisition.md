# Acquisition and read-model migration inventory

Initial source audit, 2026-11-02. R0 is in progress. `refresh-sites.json` is the
machine-readable list of timer sites produced by the audit script; it is an
inventory, not an allowlist or proof that every timer is an acquisition loop.

Direct USB update, 2026-11-04: input acquisition and committed configuration
metadata now use native CDC streams. The historical `module_usb.rs` two-second
`recv_timeout` entry is retired; source metadata wakes the parked worker. The
remaining finite reconnect waits belong to explicit configuration reconciliation,
not periodic state acquisition. See [USB metadata](usb-metadata.md) and its dated
verification. CAN input push and the other original matrix entries remain open.

November 4 audit update. Workspace discovery now enumerates serial ports on
native arrival/removal notifications. The USB fleet reads that cache during
aggregate resource passes, and a failed enumeration schedules a workspace retry.
The one-second aggregate still applies cached USB topology, retries endpoint
connections, and reads other provider models. Host-controller metadata wakes its
own resource worker on commits and first readiness.

| Surface / owner | Current boundary | Required replacement and qualification |
| --- | --- | --- |
| App/workspace/gateway | App.svelte 3 s workspace refresh | Gateway worker publishes committed status/module changes; platform serial arrival/removal notifications invalidate inventory |
| Controller input | controller-input capture + stream journal | Windows report receive publishes batches; replace Linux/macOS 4 ms backend polling with platform report streams; share journal wakeups and descriptor cache |
| Controller/device visibility | HostControllers, DeviceVisibility, visibility coordinator | Platform device/process notifications plus acknowledged visibility mutations; quiet controls remain connected |
| USB/CAN/module inputs | STM32 bounded request worker; events_state/events_read journals | Firmware/gateway unsolicited or blocking cursor subscription with boot/config fences; firmware protocol changes required; no approved sampling exception |
| Resource/system inventory | profile_resources native discovery 1 s; system_inventory cache miss I/O | Device/process/commit invalidation; collectors run off capture path, publish committed cache revision; passive subscriptions never collect/mint tokens |
| Profile library/editor/resources/ports | ProfilesPanel and editor previews | Durable document/resource commits and capture topics; revision-pinned pages, query replacement disposal |
| Runtime/activation/progress | profile_session_host health drain; activation windows 150/200 ms | Source/completion wakeups and next authored deadline; publish health/progress commits; independent Stop, compensation and heartbeat |
| Automatic selection/recovery/learning | UI status loops and native game/process tracking | Process notifications, explicit recovery/learning transitions and captured input; no authority in passive subscribers |
| Debug/topology/live/path/virtual previews | UI 250 ms–2 s reads | Subscribe to shared native snapshots and reports; virtual preview observes acknowledged output, never drives runtime |
| DCS-BIOS / managed DCS export | Socket reports plus command-pumped reads | One blocking socket producer publishes source-native batches, context and quality deadlines; separately confirm simulator effects |
| X-Plane | xplane bounded datagram poll/resubscription | Blocking socket receive; protocol subscription renewal remains timed; source expiry distinct from connection |
| Shared-memory telemetry | games/telemetry/windows_source double-copy sample | Simulator/export producer bridge must notify a coherent frame. File/process notifications alone do not indicate a changed shared-memory frame; acquisition gate unresolved |
| Games/rules/catalogues/setup | game_get_state, GameProfileRule, jobs | Publish process/context, committed rules, catalogue replacement and background job completion; explicit rescan remains a command |
| Discovery/artwork/metadata | Async workers, UI completion refresh | Worker progress/completion publishes revision, rejects old query/job generations; bounded immutable page reads |
| Companion presence/pairing/listeners | Service sockets, UI 1.5 s status | Authenticated service lifecycle events and acknowledged configuration commits |
| Panels/designer/publication | CompiledCompanionDesigner 2 s; browser App 250 ms | Immutable publication fetch once, then pushed feedback/input with exact publication generation; preserve native renderer ownership |
| Browser companion connection | Worker tick 250 ms couples feedback and liveness | Payload stream separate from lease renewal and request deadlines; backgrounding cannot stop control delivery |
| Serial/ESP32 firmware | Serial workers + status/control request cadence | Explicit producer/firmware protocol stream, bounded backpressure, publication replacement and held-release evidence per board |
| Settings/preferences/dependencies/backups | UI preferences and AI settings use native observations | Publish remaining dependency/backup worker progress. AI settings reads status after a private invalidation from listener or credential changes |
| Activity/debug/notifications | ActivityPanel/DebugPanel 3 s | Append/clear/settings commits publish pinned revisions; notification dismissal remains a real deadline |
| MCP observation/admin | Gateway commands and HTTP sessions | Shared topic registry/snapshot boundary, negotiated resource notifications and bounded cursor fallback; observe never renews authority |
| Native/browser-dev transports | gateway.ts IPC; native-gateway-bridge HTTP | Same typed envelopes, atomic subscription boundary, cancellation and overflow, including transport restart |
| Desktop/virtual/logic workers | Independent bounded pipes, host polls completions | Completion/loss wakeups; do not perform pipe I/O in capture subscriber callbacks |

The inventory includes hidden activation windows, offscreen designers and browser
workers. Timer sites must be classified during cutover: acquisition/read-model
refresh, source/protocol cadence, independent lease, deadline, debounce or animation.
No blanket timer ban: request timeouts, UI debounces, owned renewal and authored
mapping behavior remain legitimate when they do not refresh application state.

The currently supported telemetry adapters are Assetto Corsa, Assetto Corsa
Competizione, Assetto Corsa EVO, Assetto Corsa Rally, DCS-BIOS, managed DCS ownship,
and X-Plane 11/12 (see ../telemetry.md). Each of the four Assetto dialects has an
unresolved shared-memory notification boundary; a packet identifier is evidence
of a new sample only after a read, not a producer notification. A proposed bridge
must run at the simulator/export producer and publish a coherent frame with the
original packet identity. A helper that periodically copies the same pages is
still sampling and does not pass this gate. Feasibility and live qualification
must be established for each simulator separately. DCS export's existing up-to-20
Hz source-defined frame callback is legitimate upstream cadence; downstream
consumers must not turn its one-second freshness lease into a refresh interval.

Preserve controller journal session/cursor/gap semantics, batch size 4,096,
non-consuming multi-reader access, exact numeric domains and complete baselines.
Preserve companion authenticated publication/owner fences, native subscription
limits and release/backpressure behavior. Inventory/storage/discovery must not
block active output dispatch, revocation, Stop or lease maintenance.

Run `node scripts/reactive/audit.mjs --committed` to inventory the checked-out
commits across Cockpit and the sibling firmware repositories. Omit `--committed`
to include worktree changes. Use `--check` with the same mode to verify the ledger.
The scanner includes all tracked production source trees and sibling firmware
repositories. It also records native deadline waits such as `park_timeout` and
`sleep_until`. The generated list includes legitimate timing and some inline
test code, so each entry still needs semantic review. At final R9 there
must be an owner and explicit disposition for every remaining production site.
