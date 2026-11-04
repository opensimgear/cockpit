# Direct USB metadata read model

One verified CDC connection owns blocking input reception and committed runtime
metadata reception. The firmware contract is
[`module-metadata-stream.md`](https://github.com/opensimgear/protocol/blob/main/serial/module-metadata-stream.md).
The native worker parks without a refresh timer while connected and quiet.
Initial metadata baseline acquisition has one command deadline. Real transport
loss, malformed metadata, sequence gaps and counter exhaustion retire the source.

The central firmware revision covers claim/renewal/expiry, draft changes,
persistence and accepted/completed Apply, including external CAN configuration
writes. Compact absolute notices can coalesce; input edges retain their separate
ordered capture stream. A source generation change requires a fresh verified
connection. Public observations do not open ports, query configuration, claim a
writer, renew editing/output authority or access native input capture.

## Public topic

`usb.metadata.<sha256>` hashes the exact endpoint key, without normalization.
The source is `usb-metadata`; the shared broker admits UI, browser and MCP
audiences. The exact payload is:

```text
{ key, connected, port, identity, route, config, observedAtMs, error }
```

`config` is an active configuration snapshot with the required central
`revision` and `revisionExhausted: false` fields. Its ownership/dirty/pending
flags describe the runtime; its configuration body is active settings. A
disconnected snapshot may retain historical active settings for read-only
display. It grants no current editing capability. Private host client identity,
drafts, action receipts and input data are excluded.

At most four metadata bridges retain latest-only public projections. Cache and
endpoint lifecycle notifications wake them; there is no polling fallback. Source
replacement changes broker generation. Last-reader retirement, admission failure,
panic and fleet shutdown dispose the bridge. UI consumers share exact-topic
subscriptions and discard stale session/generation/revision deliveries.

## Private commands and presentation

USB mutations use `config if_revision <private-receipt-revision> <child>`.
Firmware processes actual expiry before checking the revision and executing the
child under its serialized runtime owner. A stale guard returns a current
snapshot without executing or replaying the mutation. Successful owned commands
advance the private writer receipt exactly once. A public notice or plain read
cannot advance it; unexplained revision changes revoke private writer state,
including expiry/reacquisition that reuses the same numeric owner.

Command receipts and public presentation remain separate. The USB editor
preserves unsaved local drafts, merges only public fields, waits for a matching
public revision before enabling editing, and prevents stale receipts from
restoring editing after loss/replacement. Busy commands reconcile against the
latest public state on both success and failure. A mutable editor presentation
copy does not mutate frozen observations or private receipts.

Manual and existing MCP USB commands use the same native service. The public
topic uses the shared observation admission path; full negotiated MCP streaming
and workflow qualification remain R8 work. CAN input push, other read models,
companion migration and full-app zero-refresh acceptance remain open.
