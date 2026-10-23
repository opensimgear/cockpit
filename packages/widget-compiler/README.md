# Widget compiler

The compiler accepts `WidgetSource` format 2 and emits an immutable raster widget
artifact. It runs in Cockpit or Node; companion players do not import this package.
The only product seed is `createToggleSource()`. The RPM and three-position
factories are new generalization fixtures, outside the shipped library.

```ts
const coordinator = createWidgetCompileCoordinator();
const bundle = await coordinator.compile(source, {
  width: 240, height: 144, parameters: { label: 'POWER' },
  capabilities: BROWSER_WIDGET_CAPABILITIES,
  onProgress: progress => console.log(progress),
});
// Call coordinator.invalidate() immediately after any source/parameter edit.
```

`preflightWidget` validates source, enumerates finite appearance combinations and
checks numerical accuracy and work limits before rasterization. Numeric visual
signals require full bounded sampling domains, accepted geometric error and
declared maximum pixel travel. The compiler reports the conservative half-sample
error at the chosen native size. Boolean/enum domains cover every value. Original
behavior signals remain independent of quantized visual indices.

`sourceToSvg` is an authoring-only preview boundary. Compilation feeds that
bounded projection to exact-version `@resvg/resvg-wasm` 2.6.2 with the repository's
two pinned TTF buffers. Font digests are checked before use; no system font lookup
or browser Canvas typography participates. Both Node and Cockpit load these same
resources. Changing a font or rasterizer requires a compiler identity update and
new golden pixels.

Source PNG/JPEG/WebP imports undergo complete pinned jSquash WASM decoding before
rasterization. PNG additionally checks chunk CRCs and its bounded zlib stream,
including Adler checksum and exact scanline size. Decode warnings are failures.
The decoded source is normalized to PNG with the pinned PNG encoder, so resvg's
image-format differences cannot silently produce an empty WebP surface. This
normalization leaves the original source/hash intact. No browser or OS image
decoder supplies compiler pixels. Normalized image/cache/embedding bytes are
bounded. All decoder and resvg WASM binaries are SHA-256 checked before use.

Finished frame surfaces use 16 × 16 pixel tiles. Each tile is exactly 768 bytes:
512 bytes RGB565 little endian, followed by 256 straight-alpha bytes in row-major
order. The compiler unpremultiplies resvg pixels before color quantization. Pixels
outside partial edge tiles and fully transparent RGB values are zero. Identical
finished tiles deduplicate across every state and animation frame. Dictionary
assets pack multiple tiles, split on tile boundaries at the advertised maximum
asset size, with full SHA-256 descriptors and per-tile hashes. No tile represents
an authoring layer. The artifact includes no source scene, SVG, font layout or
drawing commands.

Frames have row-major dictionary index maps and static selectors over finite
signal values and named regions. Motion tracks bake numeric visual properties
and transforms into additional complete frames. Explicit signal/region selectors
identify routes; interruption policy accompanies their ordered frame sequences.
Numeric smoothing uses the existing sampled frame family. Audio accepts bounded
PCM16 WAV and emits content-addressed assets plus semantic cue policies. Motion
or audio disabled by capability/preference emits explicit fallbacks and omits
the corresponding media. Static semantics remain identical.

Resource fields have normative arithmetic checked independently on admission:

- `transferBytes`: binary dictionary and media payload bytes, before base64 or
  session-envelope overhead.
- `frameMapBytes`: 32-bit tile indices for every cell of every frame.
- `manifestBytes`: exact UTF-8 canonical manifest bytes, including the resource
  report itself, calculated to a fixed point.
- `audioWorkingBytes`: sixteen times the largest audio payload, allowing four
  concurrent pending voices and encoded input, driver copies and float32 output.
- `residentBytes`: `transferBytes * 3 + dictionaryBytes + manifestBytes * 4 +
  frameMapBytes * 2 + width * height * 8 + 768 + audioWorkingBytes`. This allows
  base64 payload strings, a binary dictionary, manifest and numeric-index storage,
  decoder and Canvas RGBA surfaces, tile scratch and bounded audio working data.
- `activeCandidateBytes`: twice `residentBytes`, checked against target memory.

These are conservative storage allowances, rather than measurements of a
JavaScript engine's object or allocator overhead. Browser profiling and actual
runtime qualification remain separate evidence.

Validation checks complete asset closure, hashes, typed ports/neutral emits,
static selector coverage, bounds, resource totals and media policy before a
player activates an artifact. `createCompiledPixelDecoder` retains decoded
dictionaries and copies finished pixels into one reusable RGBA surface. Its
returned array can be passed directly to `ImageData`; later calls overwrite the
same array. Dispose the decoder on publication replacement.

Run `pnpm --filter @opensimgear/widget-compiler coverage` for the headless compiler
suite. Real WASM rasterization, fixed Toggle goldens, RGB565/alpha/padding/seams,
resource admission, malformed source/media, determinism, state combinations,
numeric sampling and compile cancellation are covered.
