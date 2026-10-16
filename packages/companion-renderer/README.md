# Downloadable companion rendering

This package implements the package ABI, recipe compiler, artwork preparation and host reference renderer from [the ecosystem plan](../../docs/companion-render-packages.md). Existing Designer schema 2 publications and companion protocol v1 remain supported. Native delivery, cache, source admission and the C renderer live in the service and ESP32 app; physical board timing, heap and display qualification are separate acceptance gates.

The package has four immutable objects: source semantics, scene, RGB565/A4 artwork and subset A4 fonts. The canonical integer-only manifest binds their exact SHA-256 bytes, trusted source revision/fingerprint, compiler identity and target profile. verifyPackage re-derives references, geometry, expression intervals, spatial membership, dirty tiles, blend work and simultaneous RAM subdivisions before accepting bytes. See [ABI.md](ABI.md).

compileLegacyPublication(publication, recipe, profile) verifies the schema 2 source publication and derives semantic controls, output order, types, ranges, units and interaction owners from that source. The resolved recipe supplies presentation. Initial native lowering admits momentary buttons and numeric/boolean feedback; unsupported transforms fail explicitly. Numeric source min/max must be exactly representable as Q16.16 for this initial adapter (0.5 is supported; 0.1 is rejected), preserving endpoint checks without silent range narrowing or clamping. Multiple visual nodes can consume one output variable. A feedback alias is not an additional native channel binding.

compileResolvedPanel(source, semantics, recipe, profile) supports independently checked semantic lowering adapters. Its caller-supplied descriptor is not native authority. The native service still compares every admitted gameplay field with the immutable source before enabling delivery or input. packBinaryObject and packPackage are lower-level deterministic APIs. All async boundaries snapshot caller-owned bytes/profile/source data; returned binary buffers remain caller-owned copies.

The fixed renderer supports RECT, LINE, POLYLINE, IMAGE565, MASK4, IMAGE565_MASK4 and TEXT, including formatted numeric text, bounded expressions, local pressed/availability/freshness/connection state and page-clock timelines. Local states never infer simulator confirmation or generate controller input. Sunton and Waveshare have distinct feature/resource profiles and independent layouts; a composition need not fit both screens.

prepareRgba, prepareA4, prepareGlyph, packImages and packFonts accept typed pixels and rasterized glyphs. No PNG, TTF, shaping engine, downloaded script or DOM runs on the device. The starter font is Rajdhani SemiBold under SIL OFL 1.1, with [the license](assets/OFL.txt), original font and pinned preparation metadata included. test/prepare-fonts.py uses Pillow 12.3.0 to regenerate the checked subset catalog offline; normal compilation and rendering do not depend on Python. License notices travel inside the font object's opaque pixel section after referenced glyph data, and beside each PoC package.

createReferenceRenderer is a host oracle with independent complete-frame rendering and hit testing. Checked [PoC packages](test/poc/cases.json) contain original A/B compositions for both profiles, valid source publications, manifests and all four binary objects. Eight states per composition cover offline/live/pressed/confirmed/stale/repeat/disconnect/reconnect. The native C renderer and TypeScript reference produce identical RGB565 bytes and hit results for all 32 frames. Compressed .rgb565.gz files are test transport only; devices read raw row-addressable object pixels.

The Sunton PoC uses 328–329 KiB of object content, at most 15,872 dirty pixels and a 24 KiB internal RAM claim. Waveshare uses approximately 1.04–1.05 MiB, at most 71,168 dirty pixels and a 32 KiB claim. RAM claims include the engine's fixed dual 256-node snapshots, tile and transfer scratch. These are checked accounting figures, not measurements of board headroom or performance.

Run with Node 24 and the workspace TypeScript:

    pnpm --filter @opensimgear/companion-renderer check
    pnpm --filter @opensimgear/companion-renderer coverage
    node packages/companion-renderer/test/build-fixtures.mjs
    node packages/companion-renderer/test/build-poc.mjs
    python3 apps/companion-esp32/test/scene.py

Regeneration intentionally changes exact content identities when artwork, recipes, profiles or ABI meanings change. Review generated diffs and run TS/Rust/C checks together. Firmware scene activation, streamed content slots, authenticated management and source ACK handling remain independently bounded and fail closed.