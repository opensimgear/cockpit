# Downloadable companion render contracts

This package implements the first render-package contract and deterministic packer from [the ecosystem plan](../../docs/companion-render-packages.md). It leaves existing Designer schema 2 publications and companion protocol v1 unchanged. It is a foundation for downloadable content, not an enabled content-delivery path or a physical-board proof of concept.

Implemented:

- Canonical integer-only manifest bytes, exact SHA-256 object/package identities and converter/profile identity binding.
- Explicit little-endian object headers/directories and fixed records for semantic descriptors, scenes, images and fonts. See [ABI.md](ABI.md) and `src/abi.ts`.
- Bounded structural readers, UTF-8 validation, cross-object references, row-addressable RGB565/A4 images and nonoverlapping A4 font atlases with a '?' fallback.
- Re-derived counts, complete spatial membership over reachable geometry, dirty-tile and alpha pixel-layer budgets, and checked simultaneous snapshot/tile/transfer memory accounting.
- Checked Q16.16 reference expressions, conservative interval and boolean proofs, and bounded step/linear local timelines. Expressions never execute gameplay input.
- Separate proposed Sunton and Waveshare admission profiles and precise feature/resource diagnostics. Neither initial profile supports toggle, guard, XY or triangle interactions; adding a source widget does not silently change that qualification.
- Real canonical/binary golden vectors for both device geometries, shared with the native Rust/C structural readers.

`packBinaryObject(kind, sections)` accepts already lowered records. `packPackage(source, objects, profile, memory)` validates them and produces immutable content identities; `verifyPackage` verifies received bytes, exact profile identity and optional current memory headroom. Binary buffers returned by these APIs are caller-owned copies; mutating them requires recomputing identity and passing verification again. A normal `Map` is returned for interoperability, so callers should retain the checked bytes under the content-addressed identity rather than treating JavaScript object identity as an integrity guarantee.

Hash/structure validation does not authorize a package. The native service must compare prepared semantic descriptors against its trusted source publication before activation or session creation. Runtime framebuffer/PSRAM/stack/driver headroom, clipping, stale-state handling and measured rendering timing remain firmware responsibilities. Declared RAM figures are checked accounting, not a measurement of available hardware memory.

Recipe compilation, image/font conversion, source-to-descriptor lowering, native cache/authority admission, C scene execution, flash content slots, management protocol v2 and designer integration are later milestones. The synthetic 2×2 fixture pixels/glyph demonstrate byte correctness; they are not the reference panel artwork or a visual-quality claim.

Run with Node 24 and the workspace's pnpm/TypeScript:

```sh
pnpm --filter @opensimgear/companion-renderer check
pnpm --filter @opensimgear/companion-renderer coverage
```

`node test/build-fixtures.mjs` regenerates intentional ABI vectors. Review fixture diffs and rerun Rust/C golden checks whenever changing the ABI. Sunton and Waveshare have distinct scene objects because their physical clip rectangles differ; `waveshare-scene.bin` accompanies its manifest. Profile JSON hashes exclude runtime heap readings by construction.

Before R2 scene execution, two draft contracts must be resolved explicitly: local-state variable sources (including pressed, availability/freshness and connection state) and timeline start/restart/repeat/cancellation semantics. The present variable records describe semantic feedback outputs, and bounded timeline records alone do not define animation lifecycle. See [the unresolved requirements in ABI.md](ABI.md#unresolved-scene-runtime-requirements-before-r2). These capabilities are not implemented or implicitly enabled by this foundation.
