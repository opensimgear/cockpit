# Downloadable companion rendering implementation plan

Status: implementation in progress, 16 October 2026. This extends the companion ecosystem with downloadable rendering packages for ESP32 screens. Cockpit delivers artwork, fonts, layout and rendering recipes when a panel changes; normal operation sends typed variables, input events and session traffic. New widgets composed from the installed operations require a content publication rather than new firmware.

Widget compatibility is explicit. A widget may support browser and mobile only, the Waveshare only, or both embedded profiles. The designer filters and diagnoses compatibility against the selected target. It does not silently remove widgets or replace them with simpler ones. Authors can provide alternate layouts or separate panels where useful; universal widget support is not a requirement.

The complete plan is tracked in [GitHub feature #69](https://github.com/opensimgear/cockpit/issues/69). Each implementation issue has its own GitHub milestone:

- [R0 Board baseline and correctness #70](https://github.com/opensimgear/cockpit/issues/70) — [milestone](https://github.com/opensimgear/cockpit/milestone/1), in progress.
- [R1 Contracts and golden vectors #71](https://github.com/opensimgear/cockpit/issues/71) — [milestone](https://github.com/opensimgear/cockpit/milestone/2), in progress.
- [R2 Compiler and scene interpreter #72](https://github.com/opensimgear/cockpit/issues/72) — [milestone](https://github.com/opensimgear/cockpit/milestone/3).
- [R3 Content storage and host delivery #73](https://github.com/opensimgear/cockpit/issues/73) — [milestone](https://github.com/opensimgear/cockpit/milestone/4).
- [R4 Designer and ecosystem integration #74](https://github.com/opensimgear/cockpit/issues/74) — [milestone](https://github.com/opensimgear/cockpit/milestone/5).
- [R5 Interaction catalog extension #75](https://github.com/opensimgear/cockpit/issues/75) — [milestone](https://github.com/opensimgear/cockpit/milestone/6).
- [R6 Physical proof of concept #76](https://github.com/opensimgear/cockpit/issues/76) — [milestone](https://github.com/opensimgear/cockpit/milestone/7).

## Implementation decisions

1. Keep logical controls, feedback channels, target bindings and session ownership common across companions. Add a versioned presentation model for compositions and keep compiled device packages separate from source publications.
2. Extend the existing portable C tile compositor for the first implementation. Keep board drivers behind their current pixel-write and present boundaries. An LVGL backend can be evaluated later against the same package semantics; it is not a prerequisite.
3. Prepare graphics at publication time. Bake complex decoration, resolve layout, subset fonts and lower expressions before delivery. Embedded execution has fixed memory, bounded work and no arbitrary scripting or runtime layout.
4. Stream content into an inactive flash slot before creating an input session. Activate only a verified package; then establish a fresh session, neutral baseline and feedback snapshot.
5. Preserve existing schema 2 publications and companion protocol v1. Introduce explicit schema and protocol versions for the new path, including USB negotiation.
6. Qualify features and resource budgets separately for each board. Unsupported widgets produce actionable publication errors. Presentation targets below are proposed acceptance gates to measure, not existing performance claims.

The first installation needs new engine firmware and a content partition on each board. Subsequent compatible widget and panel changes use content delivery. A new drawing operation, interaction class or hardware driver can still require firmware.

## Current code and prerequisites

| Boundary | Current implementation | Planned change |
| --- | --- | --- |
| Authoring | `packages/companion-designer/src/{types,model,assets}.ts`: schema 2 with strict field validation and immutable publication hashes | Add explicit schema 3, component recipes and target compatibility; preserve schema 2 validation and hashes |
| Logical contract | `packages/companion-contracts/src/{contracts,session}.ts`: PortableUi schema 1, typed controls/feedback and bounded sessions | Reuse existing primitive controls and session rules; add separately versioned render contracts |
| Browser and mobile | `packages/companion-ui/src/{controller,presentation}.ts` | Render composite presentation and share input/freshness rules |
| Native authority | `crates/companion-service/src/{publication,hub,feedback,server}.rs` | Validate schema 3, admit compiled packages and provide authenticated bounded reads |
| USB | `crates/companion-serial/src/lib.rs`, `apps/companion-esp32/network/serial*.c` | Negotiate the new management protocol; reuse existing link ownership and cleanup |
| Embedded graphics | `apps/companion-esp32/core/{companion,raster}.{c,h}` | Replace widget-specific drawing with a validated scene and property bindings |
| Embedded delivery | `apps/companion-esp32/network/{publication,client,wire}.*` | Add streamed package loading without enlarging current full-document buffers |
| Persistence | `network/storage.c` persists installation settings in NVS | Add a raw content store; keep credentials and installation settings in their existing store |

The current renderer already composes 64 by 8 RGB565 tiles and tracks damaged widgets. Its raster structure contains both a 1024-byte tile and a 512-byte scratch buffer. The current live decoder rejects assets and groups and supports five fixed widget kinds, four pages and 32 widgets. The new path is an engine extension, not a larger JSON panel passed to that decoder.

Fix Waveshare framebuffer coherence before evaluating richer graphics. The current adapter updates the inactive framebuffer and swaps it, without copying unchanged pixels into the recycled buffer. Begin with a full front-to-back copy after the existing safe presentation fence; optimize damage repair only after an alternating-disjoint-regions regression passes. Preserve drawing snapshots outside the network-state mutex and synchronous consumption of caller pixel buffers.

Instrument Sunton SPI time and touch-bus wait time before increasing graphics load. Its current 24 MHz display path uses synchronous 64-byte FIFO transfers. Batch transfers or introduce a fenced DMA staging buffer if measurements require it; never reuse a staging buffer before completion or hold the shared bus through an unbounded full redraw.

## Starter widget catalog

Use the supplied reference panel as a visual and interaction brief. Build original theme artwork: thin illuminated borders, restrained cyan/white feedback, red warning patterns, readable condensed labels and clear pressed states. Keep game logos and source artwork outside the initial asset requirements.

Compatibility below is the initial qualification scope, not a claim that a widget is technically impossible on another board. Native browser embeds and other DOM-specific widgets can remain browser/mobile-only.

| Widget family | Semantic behavior and variables | First qualified targets | Implementation stage |
| --- | --- | --- | --- |
| Illuminated button, outline square, module key, reset button | Existing momentary logical button; local press, availability and optional confirmed indicator | Browser/mobile, Sunton, Waveshare | First PoC |
| Warning action with striped surround | Same button semantics; prepared warning artwork and optional bounded blink | Browser/mobile, Sunton, Waveshare | First PoC |
| Framed group, section label, divider and icon | Presentation only; no logical input | Browser/mobile, Sunton, Waveshare | First PoC |
| Numeric readout, bar, annunciator and needle gauge | Existing typed feedback channel; unit/range, source age, provenance and availability | Browser/mobile, Sunton, Waveshare, within profile budgets | First PoC |
| Tabs and page navigation | Local page selection; cancel active gesture before changing page | Browser/mobile, Sunton, Waveshare | First PoC |
| Sliding toggle and two-position mode selector | Existing toggle button or selector-axis semantics, explicitly selected by author; confirmed state separate from requested state | Browser/mobile, Waveshare; Sunton after its input qualification | Interaction extension |
| Guarded emergency control | Explicit unlock-then-press policy, expiry and reset rules; gameplay input only from a deliberate user action | Browser/mobile, Waveshare initially | Interaction extension |
| Shield XY pad and center reset | Two existing bipolar axes, one pointer and atomic two-axis changes; authored fallback and centering | Browser/mobile, Waveshare initially | Interaction extension |
| Triangular power distributor and reset | Three existing unipolar axes; projected barycentric coordinates sum to one, atomic three-axis changes; authored fallback | Browser/mobile, Waveshare initially | Interaction extension |

The triangle is a composite interaction over three existing axes, not three independent sliders. Define vertex order, closest-point projection onto the triangle, edge/corner behavior, quantization tolerance and reset fallback in the interaction contract. Do not infer actual game power distribution from the requested pointer position.

The screenshot's entire tablet layout is not a required Sunton layout. Select widgets that fit that target, or author a different panel. If a target variant retains the same logical control graph, it may rearrange controls across pages. If it omits controls or changes behavior, publish a distinct semantic panel and reconcile its mapping; do not disguise omission as styling.

## Source model and compatibility

Add DesignerDocument schema 3 with a versioned component catalog and explicit semantic interaction descriptors. Preserve PortableUi schema 1 control/output IDs, logical targets and bindings where existing primitives suffice. A component instance resolves to an immutable recipe version and asset digests at publication. Mutable references such as `latest` are not allowed in published content.

The source model separates these concerns:

| Concern | Examples | Compatibility effect |
| --- | --- | --- |
| Logical controls | Button, bipolar/unipolar axes, hats and their fallback values | Mapping capability |
| Interaction | Momentary/toggle, XY coordinate mapping, triangle projection, guard policy, reset action | Mapping capability |
| Feedback contract | Channel, value type, range, unit and provenance policy | Feedback capability |
| Presentation recipe | Layer graph, geometry, artwork, glyphs, visual transitions | Styling revision |
| Target requirements | Operations, interaction features, geometry and resource estimates | Device compatibility |

Keep the schema 2 publication/fingerprint algorithm unchanged. Implement and test a schema 3 fingerprint over semantic IDs, input/output types and ranges, target bindings and interaction behavior. Exclude visual recipe, fonts, artwork and layout. Migration from schema 2 is explicit and creates a new source revision; archived publications are never rewritten under their old hashes.

Each catalog entry declares required features such as `scene.image565`, `scene.mask4`, `input.momentary`, `input.xy` or `input.barycentric3`, minimum touch geometry and estimated rendering/storage costs. The device reports compiled features, board identity, screen geometry and runtime resource headroom. Cockpit checks both individual widgets and aggregate page/package costs. A chip family name alone is insufficient evidence of compatibility.

Hash the stable profile definition and supported feature set, excluding changing free-heap readings, session IDs and network addresses. Runtime headroom is an admission check, not a reason to invalidate an otherwise reusable cached package.

PortableUi schema 1 allows one placement per primitive. A schema 3 composite owns each control/output reference once; its reset/center affordance acts on that owner rather than placing the same axes in another widget. Frames and navigation have no gameplay primitive references. Generate one validated logical page projection, and keep richer visual instances and target arrangements in the schema 3 presentation instead of duplicating PortableUi page items.

For incompatible content, report the exact widget, unmet feature or budget, required amount and available amount. Keep the existing panel running. An author may explicitly choose a supported variant; the compiler never silently degrades content. A package that fits flash can still fail on drawing time, touch geometry or internal RAM.

## Package format version 1

Represent a package as one canonical UTF-8 JSON manifest plus four immutable binary objects: `semantics.bin`, `scene.bin`, `images.bin` and `fonts.bin`. Omit unused image/font objects. Store and transfer objects independently; there is no ZIP parser, runtime PNG/JPEG decoder or base64 asset bundle on the embedded content path. Base64 is only a transport encoding for small chunks.

Package identity is `sha256:` plus SHA-256 of the exact canonical manifest bytes. The manifest contains object byte sizes and hashes but does not contain its own hash. Use the existing recursively sorted-key canonical JSON rules and integer-only manifest quantities. Object hashes cover their exact bytes. SHA-256 supplies integrity; existing authenticated host authority supplies authorization.

Required manifest fields are:

| Field | Meaning |
| --- | --- |
| `formatVersion` | Integer `1`; unknown versions fail before allocation |
| `rendererAbi` | Integer `1`; operation and property semantics |
| `sourceUiId`, `sourcePublicationHash` | Original portable panel and immutable authored revision |
| `semanticFingerprint` | Versioned schema 3 capability fingerprint, or preserved schema 2 fingerprint for the legacy adapter |
| `semanticDescriptorHash` | Exact digest of the prepared `semantics.bin` object, checked before activation |
| `targetProfileId`, `targetProfileHash` | Exact compatible board/render profile and canonical profile digest |
| `compilerId`, `compilerVersion` | Converter implementation and version; included in package identity |
| `viewport` | Physical width/height and content rectangle after the system status area |
| `requires` | Sorted required feature IDs; all must be available |
| `counts` | Pages, nodes, variables, bindings, hit regions, glyphs and timelines |
| `resources` | Actual object bytes, active scene/snapshot bytes, buffer/cache requirements and worst declared work per page |
| `objects` | Kind, version, exact byte size and SHA-256 for each binary object |

Illustrative abbreviated manifest, with explanatory digest placeholders:

```json
{
  "formatVersion": 1,
  "rendererAbi": 1,
  "sourceUiId": "ship-systems",
  "sourcePublicationHash": "sha256:<source-revision>",
  "semanticFingerprint": "sha256:<semantic-contract>",
  "semanticDescriptorHash": "sha256:<semantic-descriptor>",
  "targetProfileId": "waveshare-lcd7-s3-render-v1",
  "targetProfileHash": "sha256:<profile>",
  "compilerId": "osg-render-compiler",
  "compilerVersion": "1.0.0",
  "viewport": { "width": 800, "height": 480, "content": [0, 24, 800, 448] },
  "requires": ["input.momentary", "scene.image565", "scene.mask4", "scene.text"],
  "counts": { "pages": 2, "nodes": 96, "variables": 12, "bindings": 28, "hitRegions": 8, "glyphs": 128, "timelines": 2 },
  "resources": { "objectBytes": 1048576, "internalRamBytes": 28672, "psramBytes": 65536, "sceneRamBytes": 16384, "snapshotBytes": 8192, "tileBytes": 1024, "transferBytes": 2048, "cacheBytes": 65536, "maxPageDrawOps": 192 },
  "objects": [
    { "kind": "semantics", "version": 1, "bytes": 4096, "sha256": "sha256:<semantic-descriptor>" },
    { "kind": "scene", "version": 1, "bytes": 16384, "sha256": "sha256:<scene>" },
    { "kind": "images", "version": 1, "bytes": 995328, "sha256": "sha256:<images>" },
    { "kind": "fonts", "version": 1, "bytes": 32768, "sha256": "sha256:<fonts>" }
  ]
}
```

These placeholders are not valid wire hashes. The schema and golden fixtures in milestone 1 must use real hashes and independently validate every resource claim. Firmware derives counts and access bounds from binary contents rather than trusting the manifest's estimates.

### Binary representation

Use explicitly decoded little-endian integers, four-byte-aligned tables and relative object offsets. Never serialize pointers, native C structs, compiler padding or `double` values. Each object begins with a 32-byte header: magic[4] (`OSGC`, `OSGS`, `OSGI` or `OSGF`), version u16, headerBytes u16, objectBytes u32, directoryOffset u32, directoryEntries u16, flags u16 and twelve reserved zero bytes. Each 16-byte directory record is kind u16, recordBytes u16, offset u32, count u32 and length u32. Reject unknown flags/sections, overlapping sections, overflow and inconsistent lengths.

`semantics.bin` contains the compact source control/output descriptors, types, ranges, units, fallbacks, interaction ownership and atomic group membership. The host derives and validates it against the source publication. The device compares its scene references against this object before initial rendering and commit, without loading the entire authored document or starting an input-session lease.

`scene.bin` contains page, ordered node, variable, property-binding, bounded expression, timeline, interaction, string and spatial-tile index tables. Define fixed record layouts in the ABI specification and a shared generated constants file during milestone 1. Use integer physical coordinates after host layout; property arithmetic uses checked signed fixed point. The compiler checks ranges before conversion and the runtime rejects overflow/non-finite variable conversion. Do not reinterpret floating wire values as fixed-point bits.

`images.bin` contains RGB565 little-endian opaque pixels and A4 alpha planes, plus bounds/stride/index tables. A4 is linear coverage with two pixels per byte, high nibble first and zero padding for odd widths. Mask color is a node property. An optional RGB565 image plus A4 plane handles colored translucent artwork. Independent row addressing permits bounded reads of clipped regions. Version 1 uses uncompressed pixels; row-indexed compression needs a separately advertised extension and measured benefit.

`fonts.bin` contains immutable glyph metrics, codepoint lookup, advances and A4 glyph coverage. Use checked-in licensed font sources and explicit glyph subsets, including `?` and required units. Runtime strings are bounded; unsupported codepoints use the declared fallback glyph. No runtime font shaping, TTF parsing or system-font dependency is required for the initial profiles.

### Compiler and host validation

Create `packages/companion-renderer` for the TypeScript source types, validators, fixed-point evaluator, profile checks, packer and host preview reference. Keep the packer deterministic for the same normalized inputs. Use existing browser image-decoding boundaries to prepare bounded pixel data in a worker; package the desktop converter in Cockpit rather than depending on an installed Node/Python process. Pin font rasterization/conversion inputs and record converter versions. Cross-platform rasterization can produce different object hashes; cache exact resulting bytes and do not promise pixel-identical browser codec output across operating systems.

The native service independently verifies manifest/object hashes, bounds, profile budgets, source revision and semantic descriptors against its trusted publication. It does not accept a frontend-supplied mapping as authority. Store packages under the existing platform-owned application-data boundary with a content-addressed key; keep them outside archived designer publication records. Initially retain at most 64 MiB of derived packages, evicting unassigned least-recently-used packages and protecting active transfers. Return a capacity error when pinned data prevents eviction.

Flatten authoring groups and resolve layout/theme choices during compilation. Prepare assets at target size, deduplicate artwork and font glyphs and precompute spatial membership so rendering a tile visits its intersecting nodes. Index all reachable translated/resized bounds, or update membership when properties change; initial geometry alone is insufficient for moving nodes. Avoid replaying every node for every tile as scene size grows. Every input hit region uses the same compiled geometry as its visible control.

## Rendering operations and bindings

ABI 1 has a deliberately small rendering vocabulary. A component is a graph of these operations and properties rather than a new firmware widget enum.

| Operation or property | Definition | Cost control |
| --- | --- | --- |
| `RECT` | Opaque RGB565 rectangle | Integer bounds clipped to page/tile |
| `IMAGE565` | Copy an unscaled image rectangle | Row-addressable source; validated source/destination bounds |
| `MASK4` | Blend a constant RGB565 color through A4 coverage | Integer blend with specified rounding; bounded affected pixels |
| `IMAGE565_MASK4` | Blend image pixels through a matching A4 plane | Explicit profile feature and blended-area budget |
| `TEXT` | Bounded glyph run using a declared atlas and alignment | Glyph/string limits; fixed-point number formatting with precision 0 to 4 |
| `LINE` and `POLYLINE` | Bounded-width needle, marker or polyline | Fixed segment count; precomputed endpoint lookup for curved gauges |
| Clip, visibility, integer translation and image-region selection | Properties of an ordered node | Rectangular clipping; bounded parent depth; no dynamic node creation |

Bake rounded outlines, shadows, textures, complex paths and glows into artwork or masks. Nine-slice artwork is lowered to image operations. Arbitrary image rotation, SVG, blur, shader code, video and web embeds are outside ABI 1. A gauge needle can use a line and a precompiled angle-to-endpoint lookup instead of rotating a large bitmap.

Binding expressions support typed variable read, constant, clamp, linear map, compare, boolean operations, select, bounded lookup and fixed numeric formatting. Limit expression depth to eight and total evaluated instructions per update to the profile budget. Specify rounding, division-by-zero, overflow and invalid-data results in golden tests. No loops, recursion, allocation or external calls run in expressions.

Local timelines support step/linear transitions, a maximum of eight keyframes and duration 50 to 2000 ms; a separate bounded periodic timer covers annunciator blink. Track local pointer/pressed state separately from received feedback. Coalesce replaceable telemetry and cap redraw cadence, but preserve ordered button edges and atomic multi-axis updates. Feedback never generates controller input.

Invalidate old and new extents when a node moves or changes size. Merge damaged tiles and reconstruct every damaged tile from the background through all intersecting visible layers. Include stale/unknown/disconnected transitions and local timeline ticks in damage detection. The system status area is firmware-owned and remains readable when content is invalid or disconnected.

Add interaction classes in stages: `momentary` and local navigation first; then `toggle`, `guardedMomentary`, `xy` and `barycentric3` for declared supporting profiles. Use the Svelte controller's existing semantics where defined. Guard policy and coordinate-to-control transforms are semantic configuration, included in the fingerprint and validated by the host. Timeline/state recipes control appearance; they cannot bypass input admission or call native actions.

Add host admission for atomic interaction groups. When any XY/triangle member changes, require every member exactly once in the same batch, including unchanged axes. Validate the complete candidate and its group constraints before committing state or invoking the source sink. Reject partial groups and duplicates. Validate triangle reset/fallback distributions as well as pointer updates; define sum tolerance as one normalized 15-bit quantization step and use joint quantization so the integer components sum to 32767. Allocate residual quantization units by largest remainder, with vertex order breaking ties, and validate fallback through the same path. Keep coordinate projection and validation rules identical in DOM, C and Rust vectors.

`crates/mapping-host/src/companions.rs` currently splits input changes into individual mapping frames. Preserve each complete interaction group as one mapping frame and update queue reservation, coalescing, retry and sequence accounting accordingly. The mapping consumer must observe all-old or all-new group state. This guarantees atomic companion/mapping state; downstream virtual-device and game APIs need separate qualification. Reusing v1-shaped frames does not eliminate the need for this schema 3 semantic admission.

## Device resource profiles

Use these initial hard caps to write the compiler, loader and stress fixtures. Tighten them when the first instrumentation build demonstrates a smaller envelope. Raising a limit requires new profile identity and qualification, not only editing a constant. KiB and MiB are binary units.

| Resource | Sunton 3248S035R | Waveshare LCD7 S3 |
| --- | --- | --- |
| Hardware profile | ESP32, 4 MiB flash, no PSRAM, 480 by 320 | ESP32-S3, 8 MiB flash, 8 MiB PSRAM, 800 by 480 |
| Maximum accepted package including manifest | 768 KiB | 2560 KiB |
| Raw content slots | Two 1 MiB slots | Two 3 MiB slots |
| Manifest maximum | 4 KiB | 8 KiB |
| Scene object maximum | 16 KiB | 48 KiB |
| Semantic descriptor maximum | 8 KiB | 16 KiB |
| Pages and visual nodes per package | 4 pages, 128 nodes | 4 pages, 256 nodes |
| Variables and property bindings | 32 variables, 128 bindings | 64 variables, 256 bindings |
| Logical controls and active hit regions | 32 controls, 32 regions; qualified input classes only | 32 controls, 32 regions; qualified input classes only |
| Font glyphs and text | 256 glyphs; 120 UTF-8 bytes per dynamic string | 512 glyphs; 120 UTF-8 bytes per dynamic string |
| Simultaneous timelines | 4 | 8 |
| Expression instructions per variable update | 512 total | 1024 total |
| Additional internal content/render working set | At most 24 KiB, including snapshots, transfer and tile buffers | At most 32 KiB, including snapshots, transfer and tile buffers |
| Additional PSRAM scene/asset cache | None | At most 1 MiB |
| Transfer payload buffer | 2048 bytes decoded, included above | 2048 bytes decoded, included above |
| Initial animation/instrument presentation cap | 5 Hz | 10 Hz |
| Steady-update damaged pixel area | At most 16384 pixels, 32 KiB RGB565 output; larger changes spread across bounded slices | At most 25 percent of screen per instrument update initially |
| Intersecting visual layers per tile | 8, including opaque background | 16, including opaque background |
| A4 blend work per update | At most 16384 pixel-layers | At most 192000 pixel-layers |

For Sunton, keep scene tables in flash and stage only active binding/property state; replace legacy scene/image buffers where possible. Do not add a package-sized RAM allocation. For Waveshare, place optional caches and bulk tables in PSRAM; transport, synchronization and DMA requirements still consume scarce internal memory.

Resource claims distinguish internal RAM from PSRAM and include peak simultaneous use: old and candidate scene references, both property snapshots, input queues, transfer/tile buffers and loader metadata. The manifest example reserves 28 KiB internal and 64 KiB PSRAM, within the Waveshare profile; listed suballocations explain those totals rather than adding to them. A pixel-layer is one alpha blend at one destination pixel; many overlapping masks count repeatedly. Compiler validation includes worst reachable states and damage unions, not only the initial image.

The current Sunton build map has about 134332 bytes of static DRAM before dynamically created task stacks and Wi-Fi/TLS allocations. Existing main, touch and network task stack allocations are 8, 8 and 24 KiB. The 96 KiB JSON allocation cap is a ceiling, not spare heap. Existing Waveshare display allocations are 1536000 bytes of PSRAM framebuffers and 32000 bytes of internal bounce buffers, before driver overhead.

Provisional stress gates are minimum free internal heap of 32 KiB on Sunton and 48 KiB on Waveshare, largest free internal block of 16 KiB, no allocation failure and at least 25 percent task-stack headroom. Measure with TLS reconnect, parsing, flash reads/writes and rendering together. If the baseline already fails, reduce/replace legacy buffers or lower content limits before enabling the new path; do not report these gates as achieved.

The Sunton full-screen RGB565 wire-time floor at 24 MHz is 102.4 ms before transaction overhead. Its first target is 5 Hz for a bounded instrument region, not full-screen 5 Hz. Render in bus slices targeting at most 5 ms each so touch can acquire the shared bus. The Waveshare's current timings imply about 39 Hz scanout, with a three-VSYNC fence costing about 77 ms before drawing overhead. Keep that fence for the initial 10 Hz target and measure; do not promise 30 or 60 frames per second.

Target local touch-to-visible-press p95 at most 150 ms on both boards. Target poll-to-host-source-ACK p95 at most 200 ms on Sunton and 100 ms on Waveshare on the qualification LAN/direct USB setup, reported separately. These are embedded PoC gates; they do not replace the ecosystem's existing stricter release targets or claim successful virtual-controller submission. Measure feedback source age plus transport and rendering delay; record stale presentation at the existing 1000 ms threshold.

## Flash storage and activation

Add board-specific partition CSVs and update firmware packaging/installer checks together. This initial partition proposal keeps NVS and application addresses, reserves a 1.5 MiB factory app and leaves 448 KiB unallocated on each board:

| Partition | Offset | Sunton size | Waveshare size |
| --- | --- | --- | --- |
| NVS | `0x9000` | `0x6000` | `0x6000` |
| PHY init | `0xF000` | `0x1000` | `0x1000` |
| Factory application | `0x10000` | `0x180000` | `0x180000` |
| `ui_store` custom data partition | `0x190000` | `0x200000` | `0x600000` |

Preserve each MCU's correct bootloader offset and partition-table offset `0x8000`. Enforce application image fit in CI, and verify that future engine code fits the proposed factory app before adopting this layout. This layout supplies content rollback, not firmware OTA slots. Coordinate any later OTA partition design with the separately tracked OTA work before freezing a release layout.

Split `ui_store` into equal slots with checked metadata, manifest and binary objects. Each slot contains its package digest, content lengths, a monotonically ordered local commit sequence and CRC-protected commit record. Validate the complete committed slot on boot; the highest valid sequence wins. Write the commit marker last. Keep the other slot intact until a new update replaces it. Retain the old in-memory scene and package references until rendering no longer holds them.

Download only to the inactive slot. Erase at bounded sector boundaries and resume only after validating journal entries and rereading recorded chunk bytes; whole-object SHA-256 verification is mandatory at completion. Otherwise restart that object. A partial slot never becomes executable content. On corruption, use the prior complete slot or firmware diagnostic screen, with input inactive.

Flash erase/write may disturb RGB scanout even while input is inactive. Qualify the pinned IDF/XIP configuration during content transfer; provide a board-specific transfer screen/backlight or scanout pause where needed. Do not assume disarming input solves memory-bus contention. The first implementation stages all updates with the input source stopped and neutral. Background content updates during armed operation are deferred.

## Management protocol and normal operation

Negotiate `opensimgear-companion-v2` on the existing WSS listener; retain v1 for existing clients. Add an explicit version/feature choice to the USB wrapper. Do not add undeclared fields to existing strict v1 frames. Reuse existing credential verification, installation approval, assignment authority and disconnect cleanup.

| Step | Message and result |
| --- | --- |
| Describe | Device reports profile, features, screen geometry, limits and available content hashes after authentication |
| Prepare | `render-prepare` supplies assigned instance, source publication and canonical device profile hash; host returns the compatible manifest, package hash and semantic-object identity without creating an input writer |
| Read | `render-object-read` supplies package/object hash, offset and maximum decoded length at most 2048; reply repeats identities and offset, carries encoded bytes and a chunk SHA-256 |
| Stage | Device writes chunks into the inactive slot, hashes complete objects and validates binary records, prepared semantic descriptors, scene references and resource budgets |
| Activate | After validation and complete initial rendering, commit the slot and advertise the active package; touch remains inactive |
| Open | `render-open` supplies instance, source publication and active package hash; host rechecks assignment/profile and creates an unarmed session, returning only small session identity and the prepared descriptor hash |
| Baseline | Confirm the prepared descriptor hash and session identity, released touch and neutral baseline with the existing ACK rules |
| Run | Receive a complete feedback snapshot before deltas; then process existing typed feedback, heartbeat, ACK and ordered input frames |

The v2 management dispatcher can delegate admitted runtime messages to existing baseline/input/feedback validators. Bind the package hash in host session state and the open exchange; do not add it to every v1-shaped runtime object. Counter, generation, freshness, ACK provenance and lease rules remain authoritative. A profile/package replacement requires a fresh session even if the source publication is unchanged.

Bound compact semantic descriptors to 8 KiB on Sunton and 16 KiB on Waveshare. Device staging compares stable IDs, types, ranges, fallbacks, units and interaction descriptors with its verified scene table. Host validation compares the compiled artifact's semantics with the trusted source publication. If the descriptor does not fit, reject the panel for that profile. Never transfer that object after opening the input session. If assignment changes between staging and open, leave the rendered content inactive and prepare the newly assigned package.

Use one outstanding content chunk per device initially. Reject offset/size/hash mismatches, unauthorized object reads and objects not listed in the selected package. Apply per-device byte/rate limits and cancellation so a slow transfer cannot occupy the mapping clock or another client's queue. Allow one content transfer globally in the PoC; normal runtime clients remain independent. Reuse cached objects and expose cancel/resume progress.

Keep chunk requests within the existing per-reply deadline; the complete package transfer has its own progress timeout and no three-second overall deadline. Sunton USB is 115200 baud: a maximum-size 768 KiB package needs at least 68 seconds raw, or 91 seconds after base64 before envelopes and ACKs. Show bytes/rate and remaining work without claiming a fixed short install time.

Negotiate runtime byte budgets separately from content chunks. Initial Sunton USB limits are 512 decoded bytes for a low-priority feedback reply, 768 bytes for an input envelope with at most two edges, and 512 bytes for its ACK. Cap telemetry initially at 1024 decoded bytes/second and total measured encoded serial traffic at 6 KiB/second per direction; enforce the stricter rate/blocking limit. Keep only one low-priority reply in flight and give input ACKs priority. Fit full baseline and initial snapshot within 3072 decoded bytes each; hold local touch admission inactive until both startup exchanges complete. Reject source shapes whose IDs/strings cannot fit, rather than truncating them. Low-priority runtime transactions target at most 100 ms bus occupancy and total pending input must remain below its 250 ms expiry; tighten byte caps if wrapper overhead prevents this.

For Waveshare USB initially cap steady runtime frames at 4096 decoded bytes, telemetry at 16 KiB/second and low-priority transaction occupancy at 100 ms; its configured transport speed is higher. Wi-Fi profiles use the same bounded frames initially and qualify actual latency under reconnect/load. Host variable production can be 30 Hz while per-device feedback is coalesced, quantized or sent less frequently. If the existing feedback framing cannot fit a complete snapshot or safe deltas, reject the profile/panel or introduce an explicit protocol extension; do not silently split a strict atomic snapshot.

The soak must show no unintended lease disarms, not only good percentile latency: pending input below 250 ms, touch/render health within 600 ms and authenticated host ACKs within 1000 ms. Large page renders happen with input inactive and use resumable bounded slices. Do not extend leases to hide transport or drawing overruns.

After warm activation, normal traffic must contain zero scene, font or image bytes and zero repeated drawing commands. Typed variable deltas retain availability, unit/range, source age and provenance. Heartbeats, acknowledgements, input events and occasional management checks still flow. Local timers render blinking and interpolation without per-frame network updates. Historical plots, live maps or video require their own declared data-stream capability and are outside this initial variable-only guarantee.

Keep the last verified panel visible offline with unavailable/disconnected feedback and inactive input. Revocation, package replacement, page change, touch/display failure and lease expiry retain the current release/fallback behavior. Restoring content never restores an armed session or replays input.

## Implementation milestones

Deliver these as reviewable changes. Each milestone depends on the preceding contract/engine work; frontend catalog work and native/device delivery work can proceed in parallel after milestone 1 freezes the fixtures. No calendar estimate is implied by the numbering.

| Milestone | Concrete changes | Exit criteria |
| --- | --- | --- |
| 0 Board baseline and correctness | Instrument heap, stacks, tile/flush/present time, touch bus wait and source ACK timing; fix Waveshare coherence; prototype Sunton bounded transfer batching; verify partition/image fit | Both existing board builds pass; alternating-region regression passes; baseline measurements identify available headroom and achievable cadence |
| 1 Contracts and golden vectors | Add schema 3 and explicit migration; define render ABI/header/record layouts, profile schema, package schema and input transform rules; add `packages/companion-renderer` | TS/Rust hash/fingerprint vectors agree; schema 2 fixtures retain original hashes; unsupported targets and semantic changes are diagnosed |
| 2 Compiler and scene interpreter | Implement packer, font/image preparation, spatial index, binding evaluator and C scene loader/compositor; momentary input and local navigation; original starter artwork | Deterministic binary vectors round-trip; native sanitizer tests and golden raster tests pass for both viewport sizes; a sixth recipe needs no C enum/code change |
| 3 Content storage and host delivery | Add partition CSVs, content slot journal, native package cache, v2 WSS/USB management and compact semantic open; integrate installer migration | Streamed download never allocates a whole package; interrupted erase/write/commit tests preserve a valid slot; prepare creates no input session; authorization and old-client tests pass |
| 4 Designer and ecosystem integration | Add catalog compatibility badges/filtering, target preview, aggregate resource diagnostics, package progress and assignment; DOM composite rendering | Same supported semantic panel records the same input/feedback traces in DOM and embedded preview; incompatible widgets block publish/assign with exact reasons; optional variants are explicit |
| 5 Interaction catalog extension | Add declared toggle/guard/XY/barycentric interaction features, host group admission and complete-group mapping frames; qualify Waveshare first | Partial/duplicate/split groups rejected; triangle sum/fallback/corner vectors, XY cancellation and all-old/all-new mapping observations pass; Sunton advertises only features actually qualified |
| 6 Physical proof of concept | Execute the board matrix below, publish A then B on unchanged engine firmware, collect performance and interruption evidence | Each board independently meets its supported-subset gates; unsupported widgets stay rejected; evidence distinguishes screen behavior, host source ACK and game output |

Proposed file additions include `packages/companion-renderer/{src,test}`, `apps/companion-esp32/render/{package,scene,assets,bindings,content_store}.{c,h}`, board partition CSVs, `crates/companion-service/src/render_{packages,transport}.rs`, and `docs/verification/companion-render-packages/`. Exact file splitting can follow existing module conventions; these are planned modules, not implemented ones.

Retain the legacy firmware decoder/embedded diagnostic panel during rollout. New firmware reports whether it has a content partition and ABI 1 support. Cockpit offers content delivery only when that capability is present; otherwise it offers a clearly identified one-time engine install or the existing compatible legacy panel path. Do not reinterpret old persisted installation records or erase unrelated NVS as part of content updates.

## Proof of concept on both boards

### Packages and supported subsets

Build one theme family with independent Sunton and Waveshare fixture panels, original artwork and known semantic bindings. Package A on both contains a lit momentary button, warning action, section frame, numeric readout, bar/needle instrument and stale annunciator. Keep the Sunton panel within its dirty-area budget. Waveshare can add the larger instrument composition and, after milestone 5, toggle, guarded action, shield pad and power triangle. Sunton need not run those advanced widgets to pass.

After installing each board's engine once, publish package B with a new composite that was not present in A: a stacked warning tile on Sunton, and a multi-layer instrument with a status ring on Waveshare. Compose each entirely from installed operations, variable bindings and existing semantic controls. Also change a font subset or artwork and at least one binding threshold. Include a publication change and a second package revision for an unchanged source where compiler/profile changes alone require fresh admission. Record firmware hashes before and after; both must remain identical during A-to-B content changes.

### Required test matrix

Run both Wi-Fi/WSS and direct USB for each board. The initial renderer flash and partition install are setup steps; subsequent tests exercise content delivery, not flashing.

| Test | Pass condition |
| --- | --- |
| Cold install and warm reconnect | Cold transfer produces the correct panel; warm reconnect transfers no cached image/font/scene bytes; input waits for healthy rendering, released touch and baseline ACK |
| Variable-only operation | After warm activation, a 10-minute packet/accounting trace contains zero graphics/layout/draw-command bytes; values, source metadata, session traffic and inputs remain |
| New widget publication | Package B's new composition appears and works with the exact same firmware binary and no new native widget class |
| Visual quality | Native golden images pass; actual screens are inspected for typography, RGB565 banding, alpha edges, color order, alignment, clipping, stale trails and all status states |
| Rendering correctness | Alternate disjoint damage, overlap layers, move a needle, change pages and update text width; unchanged areas stay correct over repeated buffer swaps |
| Resource and timing soak | 30 minutes per board/transport with deterministic 30 Hz host variable production, per-profile telemetry/drawing coalescing and injected touch/reconnect; report p50/p95/p99 render, bus, input ACK, heap/stack and queue metrics plus worst-case lease margins; no unintended disarms |
| Power interruption | Native fault injection at every storage state; physical cuts during erase, object write, object verification and commit, at least three repetitions per phase/board; boot yields complete A, complete B or diagnostic mode, never mixed content |
| Malformed content | Reject unknown version/opcode, overflow, oversized object/decoded dimensions, invalid index/cycle, excessive expression work, unsupported feature and hash mismatch before activation |
| Authority and lifecycle | Wrong assignment/package/session rejected; revocation, link loss, touch fault, page change and render failure release only the affected source; reconnect requires fresh baseline and snapshot |
| Compatibility policy | Select the triangle/XY widget for an unqualified Sunton profile and get an explicit error; a compatible Sunton panel still publishes and runs |

Sunton-specific checks cover resistive calibration at corners/center, pressure noise, stationary hold/release, drag-out and touch access during bounded SPI slices. Waveshare checks cover GT911 stationary report cadence, finger substitution/multitouch cancellation, scanout stability during TLS/flash activity, presentation timeout latching and framebuffer coherence. Do not count resistive multitouch identification as a supported feature.

Use host-generated deterministic feedback and recording-mode input for the initial tests. Capture the fixture source and provenance; this proves rendering and host source handling. Simulator-confirmed data and successful virtual-controller/game output remain the ecosystem's separate qualification work.

### Evidence and completion

For each run save board revision, detected flash/PSRAM, firmware/app hashes, pinned IDF version, source publication and package hashes, profile/compiler versions, transport settings, payload counters, timings and heap/stack statistics. Include native golden images and physical screen photographs/video. Record failures and any reduced budgets as new profile revisions.

Extend existing tests rather than creating a parallel qualification system: companion contracts/designer/UI tests; companion-service transport, assignment and feedback tests; companion-serial tests; ESP32 `test/{run,network,serial,storage,sunton,waveshare}.py`; and both pinned-IDF board cross-builds. Add ABI fuzz/malformed vectors and native AddressSanitizer/UndefinedBehaviorSanitizer checks for binary readers, expressions, clipping and storage recovery. Keep existing 71 percent per-module coverage gates; coverage alone does not qualify hardware.

The PoC is complete when both boards independently demonstrate a supported high-detail panel, a new downloadable composition on unchanged firmware, bounded memory/timing, variable-only steady traffic, compatible session behavior and recovery from interrupted delivery. Broader widget compatibility and higher animation rates are separate profile qualifications.

## References

- [Companion architecture and session rules](companion-uis.md)
- [Designer publication and asset rules](../packages/companion-designer/README.md)
- [ESP32 runtime and current transport](../apps/companion-esp32/README.md)
- [Sunton hardware profile](../apps/companion-esp32/boards/sunton-3248s035r/README.md)
- [Waveshare hardware profile](../apps/companion-esp32/boards/waveshare-lcd7/README.md)
- [Companion release qualification](companion-release-matrix.md)
- [GameGlass component examples](https://gameglass.gg/), [layered groups](https://updates.gameglass.gg/changelog/major-forge-update-grouping-and-layers), and [variable bindings](https://support.gameglass.gg/en/articles/11034560-game-telemetry-and-hardware-monitoring)
- [Pinned ESP-IDF RGB memory and presentation guidance](https://docs.espressif.com/projects/esp-idf/en/v5.5.2/esp32s3/api-reference/peripherals/lcd/rgb_lcd.html)
