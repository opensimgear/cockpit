# Browser companion profile feedback

The browser companion now consumes feedback from accepted baseline, input,
heartbeat and explicit resync replies and passes it to the shared PanelCanvas
renderer. Feedback admission checks every native session/panel identity,
unsigned64 generation/sequence, exact typed channel binding and complete
snapshot. Deltas require their current snapshot. Duplicate frames do not renew
sample freshness or undo a local clear. Explicit resync requests visual state
only; it cannot replay a control. Disconnect removes live values immediately.

Fourteen browser tests pass: five receiver/renderer-projection cases, three actual
App plus shared PanelCanvas SSR cases, and six transport/credential cases.
Receiver coverage is 100% for lines, branches and functions against unchanged 71%
gates. Wrong panel feedback rejects the pending reply, closes the connection and
prevents queued control replay. The actual App tests verify requested provenance,
stale historical text, inactive stale indicators, disconnect and local clearing.
Opening a verified different publication replaces the input controller's document
before its fresh baseline; old control identities cannot remain in its values.
Both Cockpit/browser typechecks report zero errors/warnings; both frontend
bundles build successfully.

These are software component/transport fixtures. They do not establish a real
browser WSS/native profile delivery path, simulator-confirmed altitude, physical
device output or driver qualification; the isolated native/browser proof is a
separate acceptance step. No capability qualification flag changed.
