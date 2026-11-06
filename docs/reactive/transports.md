# Native and browser observations

## Companion pairing delivery

ESP32 Wi-Fi pairing sends one `pair-subscribe` with its pending request ID and
offer secret; the host sends one terminal approval/error on that socket.
Direct USB uses the same request, acknowledges `pair-subscribed`, and delivers
the terminal reply through the link-bound `serial-feedback` envelope. These
passive readers never authenticate or arm input. Original expiry, cancellation,
exact request/host/companion identity and subsequent authentication/baseline
remain required. See [ESP32 verification](../verification/2026-11-05-esp32-pairing-events.md).

## Application observations

The R1 implementation exposes the bounded channel through native Tauri channels
and the local development browser bridge. Commands remain request/reply operations.
Observation delivery has its own subscription, cancellation and receipt controls;
it does not refresh state through a command after a notification.

`osg-streams::Broker` owns the topic registry. Each topic declares its source and
allowed audiences. A broker admits at most 128 topics, 64 readers overall and 16
readers per client. Each reader retains at most 256 frames and 512 KiB of encoded
payload. Admission reserves metadata overhead within the 1 MiB snapshot envelope
ceiling. Changes have a 64 KiB wire ceiling. Counters cross JSON as decimal strings.

The native runtime owns one blocking worker per admitted reader, with at most 64
workers including canceled workers that have not exited. Producers publish to the
bounded channel without running a delivery callback. A worker sends one record,
then waits for that record's exact receipt. Cancellation drops the channel lease
and receipt sender and wakes the worker. A stalled sink cannot block a producer or
another reader. Session, generation and subscription identities fence old work.

Tauri derives the audience and client identity from the owned window. The main
window can observe all seven preference topics. The activation window can observe
theme and progress preferences. Navigation start, destruction and application
shutdown release leases. Observation controls cannot invoke general gateway
commands or choose another client's identity.

The local browser bridge uses one NDJSON connection for multiple logical readers.
This avoids exhausting a browser's HTTP connection pool with one permanent request
per topic. Subscribe/unsubscribe and receipt requests use separate bounded POST
controls. Same-origin, loopback, content-type and application-marker checks apply.
Native stdout delivery is bounded independently of command replies. The bridge
admits at most 16 physical connections and 64 pending logical readers and limits
unflushed delivery to 16 MiB. Receipts wait for their own output write to complete.
The bounded request replay window accepts parallel controls arriving out of order;
cancellation advances that window too, so aborted opens cannot accumulate forever.

Frontend `SharedObservations` shares exact-topic subscriptions, replays the current
value, isolates listener payloads and releases the last subscriber. It validates
source/session/generation/revision/sequence continuity and exposes loss explicitly.
Recovery uses a new baseline, not a timer that re-queries old state. The default
browser transport is shared across clients within one browser context. Independent
receipt requests do not block the multiplexed reader loop.

The first real producer publishes committed values for `preferences.theme`,
`preferences.devices-mode`, `preferences.canvas-navigation`,
`preferences.canvas-styling`, `preferences.tray` and `preferences.progress`.
Publication occurs under the persistence owner's commit lock. Initial hydration
remains explicit; subsequent updates apply the committed frame directly. Preference
row revisions, topic stream revisions and transport receipt ordinals are distinct.
Epoch fences prevent late reads or saves from replacing a newer observed value.

`preferences.remembered` carries a committed inventory revision. Consumers fetch
that single record on revision advance because the inventory can exceed the
change-frame budget. Window geometry remains owned by its native window writer.
