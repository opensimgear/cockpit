# Native ordered channel core (R1 foundation)

`crates/streams` provides a transport-neutral `Stream<Snapshot, Event>`. It does
not acquire devices or integrate the running app, and does not complete #190.
Tauri, browser-dev, companion and MCP wire adapters still need the envelope and
resource contracts in contract.md and must pass their separate qualification.

The authoritative service publishes its resulting complete snapshot together
with an ordered event or atomic batch. Serialization is bounded before the shared
lock. Subscribe captures the snapshot and registers its queue under that same
lock, giving one revision boundary. Subscribers get their snapshot followed by
later events, without consuming another subscriber's history. The service still
owns source identity, mutation ordering, quality and command admission.

Overflow clears queued events and reserves an explicit loss record outside the
data queue. That subscriber is blocked until a new atomic baseline; healthy
subscribers continue. Resynchronization and generation replacement deliver loss
before baseline, so held levels are never interpreted as occurrences. The owning
service must preserve the reducer's independent-source loss-before-timer rule;
the channel does not execute mappings or create command authority.

`recv` uses a condition variable. Publish, loss, close and cancellation wake it;
no periodic state query is required. `recv_timeout` is for a caller's actual
deadline. Dropping a subscription or using its cancellation handle unregisters
it and wakes blocked delivery. Payload decode and consumer work run outside the
producer lock. A decode failure invalidates continuity instead of silently
allowing later events through the same cursor.

Generation is supplied explicitly on publish and baseline recovery. Late old
callbacks cannot publish into a replacement generation. Transport implementations
must also fence deliveries already in flight when queries/sessions change; a
generic channel cannot validate adapter boot/config/publication identities.

Core byte limits count encoded JSON **payload** bytes, with frame/subscriber and
clock-ID limits separately bounding queue metadata. They do not prove the full
wire-envelope ceiling in contract.md. Wire adapters must account for envelope
overhead before admission and enforce process/client subscription caps across
topics. The core caps subscribers per stream; it is not a process topic registry.
An atomic batch additionally has a 4 MiB encoded-payload ceiling and 4,096 frames.
The first implementation preserves all ordered events and faults safely on
overload; eligible absolute-state coalescing is a later specialized policy.

Conformance fixtures in `crates/streams/tests/fixtures` describe snapshot/update,
relative-delta, overflow-recovery and exact-counter boundaries. These are shared
test inputs, not a claim that a JSON wire adapter already implements the contract.
Run `cargo test --locked --manifest-path crates/streams/Cargo.toml` and
`python scripts/streams-coverage.py`. Native production line coverage uses the
repository's LLVM tooling and 71% per-module floor. CI executes this with the
portable Rust matrix on Windows, Linux and macOS; local execution qualifies only
the current platform.
