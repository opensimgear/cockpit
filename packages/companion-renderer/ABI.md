# Companion render ABI 1 (implementation draft)

All multibyte values are little-endian. Signed scalars use Q16.16; booleans are zero or 65536. Strings are validated UTF-8 without NUL/control characters. Every `offset,length` string reference is relative to its object's string section. No native structs or pointers are serialized. `NONE=65535` is the absent u16 reference. Counts below zero and unknown flags/opcodes/sections are invalid. Reserved bytes must be zero. Immutable objects are hashed over exact bytes.

`src/abi.ts` is the machine-readable ID/record-size definition. The common 32-byte header has magic[4] (`OSGC` semantics, `OSGS` scene, `OSGI` images, `OSGF` fonts), version u16=1, headerBytes u16=32, objectBytes u32, directoryOffset u32=32, directoryEntries u16, flags u16=0, reserved[12]=0. Directories are ascending by section ID. Each 16-byte record has kind u16, recordBytes u16, offset u32, count u32, length u32; length=count*recordBytes. Data offsets are aligned to four bytes, ascending, nonoverlapping and immediately follow the preceding aligned range. Padding bytes are zero. No trailing bytes. Empty sections are omitted.

The reader must bound total bytes and directory entries before visiting records, and validate multiplication/addition without wrapping. The C reader can stream sections using a read callback; host buffers do not prescribe device RAM allocations.

## Semantics sections (OSGC)

| ID | Record size | Fields at byte offsets |
| --- | --- | --- |
| 1 controls | 32 | 0 idOffset u32, 4 idBytes u16, 6 kind u8 (button=1/unipolar axis=2/bipolar axis=3), 7 reserved u8, 8 targetOffset u32, 12 targetBytes u16, 14 targetIndex u16, 16 min i32, 20 max i32, 24 fallback i32, 28 interactionOwner u16, 30 atomicGroup u16 or NONE |
| 2 outputs | 32 | 0 idOffset u32, 4 idBytes u16, 6 kind u8 (boolean=1/number=2/text=3), 7 reserved, 8 channelOffset u32, 12 channelBytes u16, 14 maxTextBytes u16, 16 min i32, 20 max i32, 24 unitOffset u32, 28 unitBytes u16, 30 reserved u16 |
| 3 interactions | 24 | 0 idOffset u32, 4 idBytes u16, 6 kind u8 (momentary=1/navigation=2/toggle=3/guardedMomentary=4/xy=5/barycentric3=6), 7 reserved, 8/10/12 control indices u16 or NONE, 14 destinationPage u16 or NONE, 16 guardMs u16, 18 reserved u16, 20 reserved u32 |
| 4 strings | 1 | UTF-8 bytes |

Each logical control is owned once. Momentary/toggle/guarded own one button, XY owns two bipolar axes and triangle owns three unipolar axes. Navigation owns none and references a scene page. Group IDs for XY/triangle equal their interaction index; ungrouped controls use NONE. Semantic host authority must compare this descriptor against the trusted publication before admitting it. Structural/hash checks alone do not authorize a descriptor or an input session. Guarded/XY/triangle transforms are not qualified in the initial profiles and therefore cannot be admitted by them.

## Scene sections (OSGS)

Nodes are ordered by page then draw order. Geometry uses physical pixels; the firmware-owned status area is excluded by profile content bounds. A node's reachable rectangle must contain every supported bound state. Dynamic translation bindings are checked against that rectangle; spatial tiles index all reachable bounds. Parent nodes, rotation, scaling, heap allocation and arbitrary scripts are absent.

Every primitive is additionally clipped to its translated node rectangle, intersected with the authored clip and content rectangle. This applies to line stroke coverage and glyph bearings as well as image reads; authored points/glyph metrics never permit output beyond node bounds. The renderer must enforce this scissor when implementing ABI 1.

| ID | Record size | Fields at byte offsets |
| --- | --- | --- |
| 1 pages | 16 | 0 idOffset u32, 4 idBytes u16, 6 firstNode u16, 8 nodeCount u16, 10 background565 u16, 12 reserved u32 |
| 2 nodes | 64 | 0 page u16, 2 opcode u8, 3 flags u8 (visible bit0); 4 x i16, 6 y i16, 8 width u16, 10 height u16; 12/14 clip x/y i16, 16/18 clip width/height u16; 20/22 reachable x/y i16, 24/26 reachable width/height u16; 28 image u16 or NONE, 30 mask u16 or NONE, 32 font u16 or NONE, 34 lineWidth u16, 36 color565 u16, 38 firstPoint u16, 40 pointCount u16, 42 textPrecision u16 (0..4 for TEXT; zero otherwise), 44 textOffset u32, 48 textBytes u16, 50 maxTextBytes u16, 52 sourceX u16, 54 sourceY u16, 56/60 reserved u32 |
| 3 variables | 24 | 0 semanticOutput u16, 2 kind u8 (same as output), 3 reserved, 4 fallback i32, 8 min i32, 12 max i32, 16 maxTextBytes u16, 18 reserved u16, 20 reserved u32 |
| 4 bindings | 16 | 0 node u16, 2 property u8 (visible=1/translateX=2/translateY=3/color=4/image=5/number=6), 3 reserved, 4 firstInstruction u16, 6 instructionCount u16, 8 minResult i32, 12 maxResult i32 |
| 5 expressions | 16 | 0 opcode u8, 1 reserved, 2 operand u16, 4 value i32, 8/12 reserved i32. Constant uses value; variable uses operand; lookup uses operand=first lookup record and value=count. Other instructions have zero operand/value. See evaluator below. |
| 6 timelines | 24 | 0 node u16, 2 property u8, 3 interpolation u8 (step=0/linear=1), 4 firstKeyframe u16, 6 keyframeCount u16, 8 durationMs u16, 10 flags u16=0, 12/16/20 reserved u32 |
| 7 keyframes | 8 | 0 timeMs u16, 2 reserved u16, 4 value i32 |
| 8 hits | 24 | 0 interaction u16, 2 page u16, 4 x i16, 6 y i16, 8 width u16, 10 height u16, 12 visibleNode u16, 14 reserved u16, 16/20 reserved u32 |
| 9 points | 4 | 0 x i16, 2 y i16 (absolute page coordinates) |
| 10 strings | 1 | UTF-8 bytes |
| 11 tiles | 12 | 0 page u16, 2 tileX u16, 4 tileY u16 (tile grid origin is profile content x/y), 6 firstTileNode u16, 8 nodeCount u16, 10 reserved u16 |
| 12 tileNodes | 2 | node index u16, ascending draw order within each tile |
| 13 lookup | 8 | 0 x i32, 4 y i32 (strictly ascending x within each referenced lookup range) |

Expression instructions: constant=1, variable=2, add=3, subtract=4, multiply=5, divide=6, minimum=7, maximum=8, less=9, equal=10, and=11, or=12, not=13, select=14, clamp=15, lookup=16. Stack maximum eight; each binding ends with exactly one value. Operands use postfix order (`condition trueValue falseValue select`, `value min max clamp`). Multiplication/division/lookup round toward zero; overflow, division by zero, invalid boolean or absent/non-finite variable returns invalid, never an input event. Linear maps lower to arithmetic. Formatting is a host/reference helper (precision 0–4), not an allocating string instruction. Lookup clamps endpoints and linearly interpolates the two neighboring points. Expressions have no jump/call instructions. Total binding instruction work per update is derived from instruction counts and checked against the profile.

Color and image-selection property results are raw integer selectors (RGB565 0..65535 and an image index); numeric feedback/translation use Q16.16 and visibility uses exact 0/65536. Comparisons followed by select can choose raw color/image constants. The host proves conservative expression ranges against variable ranges before accepting declared binding result bounds; division intervals containing zero and overflow fail admission. It separately proves boolean membership for visibility. Visibility timelines require step interpolation and boolean keyframes. A lookup is charged its instruction plus twice its point count against update work, bounding reference validation/search instead of treating an arbitrary lookup as one cheap operation.

TEXT number bindings use the node's textPrecision, round nearest with halves away from zero, never emit negative zero and must fit maxTextBytes over the complete proven range. Labels/units can be separate static text nodes; the initial ABI has no string concatenation or runtime shaping.

## Image sections (OSGI)

| ID | Record size | Fields at byte offsets |
| --- | --- | --- |
| 1 images | 24 | 0 width u16, 2 height u16, 4 stride u32, 8 pixelOffset u32, 12 pixelBytes u32, 16 format u8 (RGB565LE=1/A4=2), 17..23 reserved zero |
| 2 pixels | 1 | Row-addressable pixels. Stride exactly width*2 (565) or ceil(width/2) (A4). A4 high nibble is first pixel; low nibble of final byte is zero for odd width. |

## Font sections (OSGF)

| ID | Record size | Fields at byte offsets |
| --- | --- | --- |
| 1 fonts | 16 | 0 firstGlyph u16, 2 glyphCount u16, 4 fallbackGlyph u16 (global index, must be U+003F), 6 lineHeight u16, 8 ascent i16, 10 reserved u16, 12 reserved u32 |
| 2 glyphs | 24 | 0 codepoint u32, 4 width u16, 6 height u16, 8 xOffset i16, 10 yOffset i16, 12 advance i16, 14 reserved u16, 16 pixelOffset u32, 20 pixelBytes u32 |
| 3 pixels | 1 | A4 coverage with identical nibble/padding convention to images. Glyphs are sorted by codepoint within each font. |

ABI 1 glyph coverage cannot overlap horizontally: xOffset is nonnegative and advance is at least xOffset+width. This makes a text node's pixel-layer accounting sound without a shaping engine. More elaborate overlapping/italic/glowing text is prepared as static artwork, or requires an explicitly qualified extension. Runtime text clips to the node rectangle and uses the declared '?' fallback for unsupported codepoints.

## Manifest and resource admission

Canonical integer-only JSON is sorted recursively by key. Objects appear in semantics/scene/images/fonts order, omitting unused assets. Package hash is SHA-256 of canonical manifest bytes without a self-hash. The manifest binds source revision/fingerprint, exact profile hash, converter version and all object bytes. Every byte-count, node/reference and claimed count is independently re-derived during verification. Derived work includes reachable-bound tile coverage and A4 pixel-layer unions for all dynamic bindings/timelines/hit feedback. Work claims cannot understate derived costs.

RAM declarations describe simultaneous peak use; scene/cache/snapshot/tile/transfer figures are subdivisions of internal+PSRAM totals, not additional allocations. Host validation can check the accounting and minimum ABI property/snapshot buffers, but runtime allocation headroom and measured drawing time remain separate firmware admission gates. No host packer can certify a board's free heap or qualify hardware performance.

## Unresolved scene-runtime requirements before R2

This is a tested package/record foundation and an implementation draft. The following contracts remain unresolved; the current records do not imply support for them:

- Local-state variable sources: scene variables currently reference semantic feedback outputs only. R2 must define explicit sources, ownership, typing and invalidation for local pressed state, availability, feedback freshness/provenance and connection state before recipes can bind those states. A pressed overlay must not infer simulator confirmation or emit controller input.
- Timeline lifecycle: the current records bound keyframes and duration but do not establish start/restart triggers, repetition/blink, pause/resume, page activation, disconnect behavior or cancellation. R2 must specify those rules, their resource costs and golden vectors before enabling animation in a runtime.

Do not repurpose reserved fields or enable new meanings by inference. Resolve and version any necessary ABI/profile changes explicitly, update validators and TS/C/Rust fixtures together, and keep unsupported compositions rejected until their semantics are implemented. The first C expression evaluator and structural object readers do not constitute a scene interpreter or animation admission.
