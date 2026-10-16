# Companion C3 apps and desktop targets — 16 October 2026

Issue [#41](https://github.com/opensimgear/cockpit/issues/41) now includes Windows,
Linux and macOS desktop clients alongside the Android/iOS shells. This record
separates implementation and automated evidence from native-device, second-PC,
permission and signed-distribution qualification.

## Delivered behavior

- Separate Tauri app identity, narrow native client API, local navigation/ACL and
  no Cockpit gateway/game/controller-driver dependencies. Desktop build recipes
  select native NSIS, Debian or .app development packages, record hashes and
  reject accidental host/driver dependencies.
- Bounded atomic IndexedDB cache of four verified public panels, 24 MiB total and
  at most 13 MiB each, with validated document/asset hashes and remembered page.
  Offline content remains disarmed with unavailable feedback. It stores neither
  credentials nor action queues. Corruption/quota failures have explicit reset.
- Session/view epochs reject late status/publication/arm responses after Stop,
  selection, page/orientation and lifecycle changes. Reconnect needs a new native
  session/generation, verified publication, acknowledged baseline and explicit
  Start. Feedback clears on authority loss; buffered gestures are not replayed.
- Native async protected-store initialization keeps Keychain/DBus/mobile calls off
  the main event loop. Windows uses DPAPI; Linux Secret Service and Apple Keychain
  hold a native AES-256-GCM file key. Android has a narrow app-owned Keystore
  bridge. Missing/locked stores do not fall back to plaintext. Retry restores
  valid saved identity disarmed; corruption remains inspectable and requires
  valid restoration or explicit Forget. Failed deletion preserves authority.
- Native focus/close/layout events invalidate input; Android/iOS native
  suspend/resume events use the same epoch. Windows also checks the input desktop
  before arm/input/keepalive so a secure/locked desktop cannot renew input.
  Native 600 ms UI and 1500 ms host source leases remain independent fallback.
- Fixed a C1/C2 interop bug: valid canonical JSON reorders object keys, so logical
  page comparison must use semantic canonical equality while retaining item
  array order and rejecting changed control content.
- Fixed an existing Linux output-owner release race exposed by #40 remote CI:
  an RAII lease explicitly unlocks before closing its descriptor, so an incidental
  inherited pre-exec file descriptor cannot prolong ownership. A deterministic
  inherited-FD regression covers it; no backend qualification gate changed.

Android/iOS authentication remains explicitly disabled pending #55/#56 even when
an adapter is present. Linux/macOS companion targets do not qualify or enable
their host controller/virtual-output backends. Saving, pairing or installing
content does not start game output. ACKs remain source-accepted/submitted:false.

## Local validation

| Check | Result |
| --- | --- |
| Shared designer semantic/canonical regression | 61 passed; 99.66% lines |
| App cache/session/input helpers | 21 passed; 93.88% lines, 96.19% branches, 90.67% functions |
| Individual app modules | Cache 90.11% lines; session guard and input queue 100% |
| Native service/trust/storage and real-WSS tests | 46 passed; all eight measured modules exceed 71%; persistence 93.98%, client 89.29% |
| Windows Companion native shell/lifecycle | 5 passed |
| Android native bridge portable codec | 1 passed; not Android runtime evidence |
| Windows host regression | 321 passed, one existing test ignored in the final serial run |
| Frontend Svelte/TypeScript and production build | Passed with zero diagnostics |
| Linux output-owner regression and suite | 53 passed; no virtual device activation |
| Linux service core regressions | 37 passed, one native-store cycle explicitly ignored by default; separate live attempt unavailable |

The first parallel Windows host regression encountered an existing UDP bind
access error (10013) in a DCS test; the full serial rerun passed. No DCS behavior
was changed for that retry. #40 remote CI passed Windows/macOS native, Linux host,
frontend and mapping jobs, but exposed the separate Linux owner-lock race fixed
above; that run is not represented as all green.

Final local desktop delivery checks also passed:

- Windows: native app five tests, embedded frontend build, unsigned NSIS package
  (3,932,305 bytes), standalone dependency verification and SHA-256 evidence.
- Linux under WSL: native app five tests, embedded Tauri build and verified Debian
  package (60.80 MiB). The explicit live Secret Service cycle was attempted and
  failed at availability because this WSL session has no Secret Service provider.
  The client fails closed; real-store/pairing and desktop install/display-server
  qualification remain #61.
- Package helper: four regressions pass. Linux virtual-output coverage passes
  all modules at 71% or higher; linux_uinput is 82.07% with 53 passing tests.

Development evidence files contain exact artifact bytes and SHA-256. No installer
was run and no release was published. macOS native packaging and Android debug APK
compilation are configured in CI but have not run locally. A configured workflow
is not a completed platform run; macOS/mobile native qualification stays open.

## Reproduction

From the repository root:

```text
pnpm companion:check
pnpm companion:test
pnpm transport:test
pnpm check
pnpm ui:build
pnpm companion:ui:build
cargo test --locked --manifest-path apps/companion/src-tauri/Cargo.toml
cargo test --locked --manifest-path crates/companion-platform/Cargo.toml
python scripts/companion-coverage.py --service-only --output-dir <output>
python -m unittest discover -s scripts -p test_companion_package.py -v
python scripts/companion-package.py --output-dir <packages>
python scripts/companion-package.py --verify-only <packages>/<platform>/development-evidence.json
```

Use native build prerequisites for each OS. The package helper requests
development/unsigned builds and never installs/publishes. Mobile debug builds
still require their SDK/native toolchains; iOS requires Apple provisioning.
Windows UNC source builds may use a clean local mirror with external output
directories; test results refer to that isolated committed-base plus owned
changes, preserving unrelated work from other chats.

## Open qualification

- [#55](https://github.com/opensimgear/cockpit/issues/55): Android SDK/native build,
  device Keystore/JNI, touch, lifecycle, network trust and signed installation.
- [#56](https://github.com/opensimgear/cockpit/issues/56): iOS/Xcode/native Keychain,
  iPhone/iPad, local-network permission, lifecycle and signed distribution.
- [#57](https://github.com/opensimgear/cockpit/issues/57),
  [#60](https://github.com/opensimgear/cockpit/issues/60): second Windows PC,
  native WebView persistence, lock/sleep, discovery/firewall and packaged LAN.
- [#61](https://github.com/opensimgear/cockpit/issues/61): Linux package install,
  real Secret Service, Wayland/X11, native cache/lifecycle and LAN.
- [#62](https://github.com/opensimgear/cockpit/issues/62): macOS native build/install,
  Keychain/local-network permission, lock/sleep, signing/notarization and LAN.
- [#58](https://github.com/opensimgear/cockpit/issues/58),
  [#59](https://github.com/opensimgear/cockpit/issues/59): physical mixed-peer
  load/performance, display/touch/safe-area and native document checks.

No manual or physical criterion is silently checked by these automated results.
Unqualified output/provisioning/hiding/live-script and mobile-authentication
capabilities remain disabled.

Mobile Bonjour declarations do not establish multicast discovery: mdns-sd uses raw multicast sockets, so iOS entitlement/native Bonjour and Android multicast-lock/permission behavior require #55/#56. Manual/decoded-QR pinned WSS is independent.
