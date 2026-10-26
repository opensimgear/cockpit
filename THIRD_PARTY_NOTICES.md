# Third-party materials

The repository's [all-rights-reserved notice](LICENSE) applies to original
OpenSimGear material. It does not relicense dependencies, third-party assets,
upstream library data or other separately licensed portions.

## Existing notices and provenance

- Rust dependencies are recorded in `src-tauri/Cargo.lock` and the package
  manifests under `src-tauri/` and `crates/`.
- Local MCP uses the official Model Context Protocol Rust SDK, rmcp 3.6.0
  (Apache-2.0), from https://github.com/modelcontextprotocol/rust-sdk.
- JavaScript dependencies are recorded in `package.json` and `pnpm-lock.yaml`.
- The host widget compiler uses unmodified `@jsquash/png` 3.1.1,
  `@jsquash/jpeg` 1.6.0 and `@jsquash/webp` 1.5.0 (Apache-2.0 package wrappers
  with the additional upstream codec licenses preserved in
  `public/licenses/jsquash/`). Their source is available at
  https://github.com/jamsinclair/jSquash. This software is based in part on
  the work of the Independent JPEG Group. These codecs validate source images
  and normalize them to PNG in Cockpit; companions receive finished pixels.
- The host widget compiler uses unmodified `@resvg/resvg-wasm` 2.6.2
  (Mozilla Public License 2.0). Its source is available at
  https://github.com/yisibl/resvg-js/tree/v2.6.2 and its complete upstream
  license is distributed in `public/licenses/resvg/LICENSE.txt`. This compiler
  runs in Cockpit; the companion receives compiled pixels.
- Canvas Auto-layout uses unmodified elkjs 0.12.0 (Eclipse Public License 2.0),
  from https://github.com/kieler/elkjs. Its complete license is distributed in
  `public/licenses/elkjs/LICENSE.md` with the frontend assets.
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
- No publisher artwork is bundled as an application default. Native UI tests
  retain one generic JPEG at `tests/fixtures/artwork/landscape.jpg`; its exact
  source, publisher/developer credit and supplied copyright are documented in
  `tests/fixtures/artwork/README.md`. Artwork and game trademarks remain the
  property of their respective owners and are excluded from the Cockpit source`n  license.

This file is a scope and provenance index, not a replacement for the full license
texts and copyright notices accompanying third-party material. Preserve the
applicable full notices when redistributing or packaging those portions. Existing
upstream permissions, including any commercial permissions, remain unaffected.


## Game platform icons

Steam, Epic Games, Battle.net, EA, Ubisoft and GOG brand SVG paths are from
[Simple Icons 15.16.0](https://github.com/simple-icons/simple-icons/tree/15.16.0),
provided under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
The unmodified Xbox brand SVG path is from [Font Awesome Free 7.3.1](https://github.com/FortAwesome/Font-Awesome),
Copyright 2026 Fonticons, Inc., licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
These assets identify their respective game platforms and render in the current UI color.
