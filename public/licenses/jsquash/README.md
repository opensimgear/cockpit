# jSquash image codecs

Cockpit uses unmodified `@jsquash/png` 3.1.1, `@jsquash/jpeg` 1.6.0 and
`@jsquash/webp` 1.5.0 for source-image validation and canonical PNG conversion
inside the widget compiler. They do not run in the companion pixel player.

Project: https://github.com/jamsinclair/jSquash

Each package's complete `LICENSE` and packaged `codec/LICENSE.codec.md` have
been copied unchanged into its corresponding directory. The package wrappers
identify their license as Apache-2.0. Preserve the additional codec notices;
the package license does not replace them.

This software is based in part on the work of the Independent JPEG Group.

The JPEG directory additionally contains unmodified `README.ijg` and `LICENSE.md`
from https://github.com/mozilla/mozjpeg/tree/v3.3.1, the codec source version
referenced by jSquash's JPEG build recipe. The WebP directory includes the
upstream patent grant from https://github.com/webmproject/libwebp/tree/v1.0.2,
the version recorded by its packaged codec README.

The checked-in dependency lock records the exact package archives. Compiler code
checks the SHA-256 of all three decoder WASM binaries before initialization.
