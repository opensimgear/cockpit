# HID reception and debugging contract

H0 defines what each consumer can conclude from a descriptor, report or platform capability. H1–H9
use the same [wire types](contract.ts), [construct and platform matrix](coverage.md), [authored
fixtures](../../tests/hid-contract/) and [device observations](../../tests/hid-device-captures/).
The existing controller service in `crates/controller-input` remains the baseline for observing
current behavior. Its mapping-oriented ports and latest-value projection cover a subset of this model.
Milestone dependencies and platform qualification are tracked in the [parent feature](https://github.com/opensimgear/cockpit/issues/177).

## Using the portable decoder

For Linux hardware and native package checks, follow the [hidraw receiver qualification procedure](linux-receiver.md).

The [Rust library](../../crates/hid-report/) accepts a descriptor byte slice through
`parse_descriptor`. Inspect its diagnostics before interpreting a report. Item byte offsets
and lengths refer to the original descriptor. Fields reference their declaring item and enclosing
collection, and each report kind and ID has its own payload layout.

Pass the descriptor, report kind, delivery framing and complete wire bytes to `decode_report`.
Choose `IdPrefix` for a numbered wire report, `Unnumbered` for a bare unnumbered payload,
`OsZeroPrefix` for an unnumbered delivery with a leading zero, or `OutOfBand` when the caller
supplies the ID separately. A truncated report retains its bytes and yields no partial values.
Extra delivery bytes remain inspectable beside a diagnostic.

Each value references its source field and element. Read the field's `attributes()` for wrapping
and other Main flags. Integer and selector strings retain full precision. Where linear scaling
is defined, the physical result gives an exact numerator and denominator with the descriptor's
unit and decimal exponent. The field's logical and physical endpoints provide its derivation.
The caller supplies all input bytes, so the same descriptor and delivery can be replayed offline.

Run `cargo test --locked --manifest-path crates/hid-report/Cargo.toml` from the repository root.
With the Rust `llvm-tools-preview` component installed, `python scripts/hid-report-coverage.py`
runs the corpus and generated tests and checks production coverage per source file.

## Identity and provenance

For the Windows implementation, see the [selected-interface receiver and qualification procedure](windows-receiver.md).

Publish a snapshot atomically. Within one service epoch distinguish interfaces, top-level collections
and repeated sibling collections, even on two identical units or a composite device. Bind a field to
its collection and declaration ordinal (or OS data index/element identity). Preserve the underlying
scoped identifier and parent links. A name, VID/PID, usage pair or USB serial alone cannot disambiguate
these boundaries. Keep reconnect correlation separate from authority to address the current capture.

For raw descriptors hash the exact unmodified descriptor bytes with SHA-256. For a parsed-only snapshot
hash the UTF-8 canonical JSON object with the properties `provenance`, `collections`, `fields` and
`layouts` from the snapshot. Use snapshot-local identifiers and stable declaration order or OS-provided
indices. When the OS supplies no declaration order, sort by the stable backend indices before hashing.
Use [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) for serialization. A parsed digest identifies only that
projection. Keep historical snapshots with their original references and last-observed status. A restart,
reconnect, layout change or source replacement opens a fresh generation. Fence late callbacks before
publishing the replacement. A reader holding the prior target can only inspect its retained history.

## Reading reports

Identify the layout by report kind and ID. Record the backend framing before stripping an ID or OS
placeholder for decoding. Element callbacks that supply only an element value carry unavailable raw
bytes and cannot be presented as a complete report. Preserve OS batching boundaries and the order of
individual reports inside a delivery. Timestamp at native receipt before queueing, using a monotonic
clock. When a batch has one clock reading, retain that time for all reports and order by sequence.
Record backend/device clocks separately with their units. Never infer device sampling time from UI time.

Use little-endian HID bit order with bit zero at the first payload byte. Preserve declared widths,
counts and each element index. Sign-extend according to the descriptor's logical domain; decode an
unsigned maximum as unsigned when its minimum is nonnegative. Preserve the encoded integer and raw
bits even for a null or out-of-range value. For arrays retain each selector and its resolved usages
or evidence of an unknown selection. Constants/padding remain in the report. An unrecognized usage
does not invalidate a structurally valid vendor value. Unknown report IDs, truncated input and invalid
layout produce diagnostics plus original bytes, without filling missing values from an older report.

Represent exact integers as canonical decimal strings in every JSON path, including exports and MCP.
Use checked arbitrary-precision arithmetic up to the field budget. For example, the 64 payload bits
`0100000000002080` have unsigned value `9232379236109516801`; a JavaScript Number cannot preserve
the low bit. Never infer center, neutral, deadzone or hat positions from signedness, usage name or
logical bounds alone. If a physical interpolation is meaningful, retain exact rational numerator
and positive denominator plus unit/exponent and the source bounds. Omit it for missing or ambiguous
bounds, nonlinear/array fields or zero span. Presentation conversions leave native values untouched.

Keep the ordered timeline separate from the latest absolute-state view. Equal absolute reports still
advance sequence. Two equal relative deltas or edges are two events; neither deduplication nor
automatic integration is valid. Waiting means this generation has no sample. Stale retains the sample
and its acquisition time under an explicit freshness policy. Offline ends live authority. Null is a
descriptor-defined sample state, never a disconnect or a synthetic neutral.

## Read admission, recording and replay

Enumeration uses non-exclusive discovery. Record capability and access separately from an active
subscription. Raw capture and recording require an explicitly selected interface or collection,
including when a composite controller also exposes keyboard or mouse collections. Exports retain the
selected scope and provenance. An explicit input or feature read carries an exact target,
kind, report ID, maximum bytes, deadline and request identity. Verify current generation, advertised
layout, OS permission and caller's explicit intent. A feature read can have device-specific effects:
display the requested target/report and never poll it on discovery, resume or reconnect. Limit to one
outstanding explicit read per interface and a two-second caller deadline. On timeout retire the request,
fence a late completion and keep a stuck backend call from admitting more work on that handle. Report
unsupported operation, denied OS access, busy/exclusive owner, timeout, offline, stale generation,
malformed response and capacity separately. A timeout does not justify retrying an ambiguous device read.

The writer appends descriptor references and sequence-stamped records outside the hot path. Record
gaps explicitly for observable queue loss; report unknown upstream loss separately without inventing
sequence numbers. Stop or device removal seals the retained prefix and terminal reason. Export contains
snapshots, clock metadata, ordered records, gaps, capture limits and truncation status. Validate lengths,
references, indexes and arithmetic before allocation on import. Replay uses a private virtual clock and
an observer-only sink. Seeking reloads the descriptor and reconstructs state up to the requested
sequence. It cannot acquire a device handle, submit a report, synthesize desktop input or forward FFB.
UI and MCP share one native owner, job IDs, cancellation boundaries, errors and paginated readbacks.

One native acquisition owns the sequence for an interface generation. A collection-filtered reader
retains the original sequence and identifies its filter. Intentional filtering and queue loss are
distinct. Multiple fields or collections within one report retain the same parent report identity.
A parsed element delivery identifies only the elements actually observed. On a reported upstream
loss, invalidate the absolute baseline until the backend supplies a fresh observation. Missing relative
deltas stay missing. Replay carries the recording origin and stays outside the live interface inventory.

## Budgets and milestone acceptance

The following are acceptance targets for the subsequent implementations. H0 supplies the contract and
fixtures. Report host, OS/build, backend, negotiated layout, workload duration, counts, queue high-water
marks and latency percentiles when qualifying a backend.

| Milestone | Workload and pass condition |
| --- | --- |
| H1 portable parser | At most 64 KiB descriptor and 64 KiB per report, 64 collection/global stack levels, 16,384 fields and 4,096 bits per scalar. Check count × width and aggregate layout bounds before expanding. Reject over-budget input with path/offset and reason. Repeated decode and export/import of every authored vector preserve bytes and exact values. |
| H2 Windows, H3 Linux, H4 macOS | Compare captures with independent platform evidence on each OS. Verify at least one numbered and one unnumbered layout using fixtures where hardware is unavailable. Exercise duplicate units/TLCs, denied read, disconnect, reconnect and generation replacement. Report actual hardware and fixture evidence separately. |
| H5 persistence and capture | Limit a reader queue to 4,096 reports or 8 MiB of retained data, whichever comes first; cap all reader queues at 32 MiB per service. An intentionally stalled reader produces a measurable gap or termination. Independent fast readers continue. Persist snapshots off the capture thread; per-report SQL writes are prohibited. |
| H6 inspector and recording | At most 64 MiB or 1,000,000 records per recording including metadata, whichever comes first. Seal on capacity and expose reason and last sequence. Bound decoded expansion before allocation. Import uses the same limits. Replay twice with the same clock and compare exact ordered records including gaps, relative repeats and terminal status. |
| H7 MCP | Limit one response to 128 records and 1 MiB serialized, with bounded slices for individual large records. Authenticate subscriptions and explicit reads; revoke/close releases readers. Compare UI and MCP against identical target, sequence and bytes, including unavailable reasons and stale-cursor recovery. |
| H8 vJoy | Inventory each capability of the pinned 2.2.2.0 driver/SDK. In an explicitly acquired diagnostic session preserve callback packet boundaries, unknown effects and version provenance. Verify fixture/real origins separately. Never infer visibility into another application's physical output traffic. |
| H9 integrated load | Four synthetic interfaces × 1,000 input reports/s × 64 bytes for 60 s. Fast capture must account for all 240,000 reports without gaps. Target p99 receive-to-native-reader latency ≤10 ms and p99 inspector interaction ≤100 ms during capture. Repeat with a stalled reader and full recorder, verifying bounded memory, honest loss and independent Stop within 1 s. Record platform results individually. |

SET_REPORT, physical output writes, force-feedback forwarding, normalization for routing and virtual
device provisioning remain separate product decisions. Output and Feature declarations are inspectable
even when no authorized operation can receive their traffic.

## H0 verification

On 28 November 2026 the authored corpus check passed for seven descriptors, eleven report layouts,
eighteen golden reports and a separate truncated descriptor. Thirteen tests covered the fixture
checker, including byte-for-byte reconstruction and exact values beyond JavaScript's safe integer
range. Coverage was 100% of lines, 97.67% of branches and 100% of functions. The wire types passed
strict TypeScript checking. The Windows controller-input suite passed all 180 tests on the host.

The dev Cockpit identified a VKB T-Rudder and Gladiator EVO R. The [device corpus](../../tests/hid-device-captures/)
contains passive Windows Raw Input reports and separate Cockpit snapshots of OS-parsed control state.
Neither capture showed a value transition. The captures establish real report bytes and current capability
projections. Changes under actuation, original descriptor acquisition and other platforms require
their own evidence.
