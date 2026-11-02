# Native controller observation streams

R2 begins at the existing authoritative controller registry. Native reports still
commit one atomic report, including implicit button releases, to its nonconsuming
journal. Selected-controller subscriptions now observe the same commit boundary
through `controllers.input.<local-source-UUID>`; subscribing never acquires a
device or enables outputs.

`ControllerService::observe` holds the registry lock while preparing a selected
topic and registering its reader. Subsequent reports publish a full level snapshot
for future subscribers and an ordered report frame for current subscribers.
Descriptors are retained with the source and cached in the topic snapshot; report
changes contain values and source metadata, without reacquiring or retransmitting
descriptors. Speculative metadata transaction clones have no publishers. Only the
committed metadata merge publishes to live subscribers.

There are at most 64 selected-controller bindings. New selections retire only
idle bindings under the broker's subscription lock; an old publisher cannot retire
a replacement registration. All readers retain the shared process/client limits
from the transport broker. Slow readers overflow explicitly and require a fresh
baseline. Queued button transitions are not replaced by their latest value.

The wire broker's session/revision/sequence belong to its transport topic. The
payload session/cursor/sequence belong to native capture. Wire generation is
native source generation plus one, reserving zero for offline retained records in
the native domain. Generation replacement publishes loss followed by a new
descriptor/level snapshot. Reassignment advances beyond both live identities and
the staged generation, even when a remembered source reconnects during storage
I/O. Exhaustion withdraws values rather than wrapping identities.

Capture time is the native journal clock (`controllers:<capture-session>`).
Metadata edits and late subscriptions preserve the last native capture timestamp;
they cannot renew input evidence. Partial multi-report baselines expose descriptor
metadata with unavailable values. A complete baseline restores levels and is not
an occurrence that can issue commands. Diagnostics preserve raw unsupported-field
metadata separately from routable controls. Native ranges and values are unchanged.

The host inspector and profile input preview share one frontend observation store
per selected source. They validate entire frames before applying values, retain
the descriptor for ordinary reports, withdraw values on loss and dispose the last
reader on source change/unmount. Metadata command responses cannot overwrite a
newer streamed value. Native Tauri and the browser gateway prepare the same topic
under the owner lock; the activation window remains restricted to its preferences.

Windows Raw Input capture now blocks on messages or an explicitly signaled shutdown
event. Enumeration runs at startup and on device-change notifications, with an
arrival boundary installed before processing the following input report. The old
16 ms idle wake and periodic two-second enumeration are removed. Descriptor failure
remains explicit; a timer does not silently retry collection.

This is a staged migration. Inventory/visibility refresh, profile reducer journal
polling, Linux/macOS qualification capture, USB/CAN firmware acquisition, MCP tools
and the rest of the application remain separate unfinished paths. R2 is not fully
accepted until those acquisition boundaries and the real hardware/loss tests pass.
