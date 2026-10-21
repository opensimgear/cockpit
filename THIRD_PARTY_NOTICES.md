# Third-party materials

The repository's [all-rights-reserved notice](LICENSE) applies to original
OpenSimGear material. It does not relicense dependencies, third-party assets,
upstream library data or other separately licensed portions.

## Existing notices and provenance

- Rust dependencies are recorded in `src-tauri/Cargo.lock` and the package
  manifests under `src-tauri/` and `crates/`.
- JavaScript dependencies are recorded in `package.json` and `pnpm-lock.yaml`.
- External simulator integrations and tools are described in
  `docs/simulator-integrations.md`, `integration-profiles/` and
  `simulation/dcs-bios/README.md`. References to a project or its interface do
  not imply that its code is owned by OpenSimGear.
- Any separately distributed vJoy, HidHide, DCS-BIOS or other external software
  retains its upstream terms.
- Bundled optional Windows installers for vJoy 2.2.2.0 (BrunnerInnovation) and
  HidHide 1.5.230.0 (nefarius) are unmodified upstream releases. Their provenance,
  hashes and complete upstream license notices are in
  `src-tauri/resources/dependencies/`, distributed as `dependencies/` alongside
  the application's resources.
- Short-code pairing uses RustCrypto SPAKE2 (spake2 0.4.0, Apache-2.0 OR MIT):
  https://github.com/RustCrypto/PAKEs/tree/master/spake2.
- Native ESP32 installation uses espflash 4.1.0 (Apache-2.0 OR MIT):
  https://github.com/esp-rs/espflash/tree/v4.1.0.
- Bundled firmware and other resources must retain the notices supplied with
  their source projects.
- Game tile screenshots are publisher artwork from the official Steam store
  listings. Exact image URLs, publisher/developer credits and supplied legal
  notices are recorded in `public/game-art/sources.json`. Artwork and game
  trademarks remain the property of their respective owners and are excluded
  from the Cockpit source license; see `public/game-art/README.md`.

This file is a scope and provenance index, not a replacement for the full license
texts and copyright notices accompanying third-party material. Preserve the
applicable full notices when redistributing or packaging those portions. Existing
upstream permissions, including any commercial permissions, remain unaffected.
