# OpenSimGear Companion

A separate **Tauri 2, Svelte 5 and TypeScript** panel app for Windows, Linux and macOS, with Android/iOS shells. It uses Cockpit's shared canvas and connects as a narrow authenticated client to the same host endpoint. It does not link Cockpit's gateway, game-launch or controller-driver services, and needs no vJoy/HidHide on the client computer.

## Pair and run a panel

1. Publish a panel in Cockpit, enable its Companion listener on the intended interface/port, and make the immutable revision available.
2. Transfer the host's pairing payload directly to this app. Discovery is an untrusted hint; the native client checks the out-of-band CA fingerprint and ordinary TLS chain/SAN/validity/signatures before sending a one-time secret.
3. Request pairing and approve/assign the installation in Cockpit. Refresh assignments, select a controller instance and Connect. Verified layouts/assets and a fresh acknowledged neutral baseline must arrive before **Start panel**.
4. Start explicitly. Stop, page/orientation/layout change, focus/close/background loss, native invalidation, stalled UI, network failure or overflow disarms this instance. Reconnect and Start explicitly; old actions are never replayed.

Source acknowledgments say `source-accepted` with `submitted: false`. They do not claim successful virtual output or a game action. Existing unqualified host output, provisioning, hiding and script gates are unchanged.

## Offline content and recovery

The app atomically caches up to **four verified public panels, 24 MiB total, at most 13 MiB per entry**, including raster assets and the last selected page. Every disk read revalidates the document, capability/content hashes and assets. Credentials and input/action queues never enter IndexedDB. Quota/corruption errors leave online use available; **Clear saved panels** explicitly resets the public cache.

Use **Open offline** to view a saved panel after disconnection/restart. Controls remain disarmed and feedback is unavailable. A cached panel grants no authority: Connect again for a new generation/baseline and explicitly Start. Feedback is cleared on epoch/selection/lifecycle loss. Late asynchronous responses cannot rearm an obsolete view.

Native credential adapters are distinct from this public cache:

| Platform | Native protection | Qualification boundary |
| --- | --- | --- |
| Windows | Current-user DPAPI | Local native transport/store tests; physical second-PC, lock/sleep and signed install remain #57/#60. |
| Linux | Secret Service holds the AES-256-GCM file key | Requires a running, unlocked Secret Service; missing/locked service fails closed. Native package/display-server/store/lifecycle qualification remains #61. |
| macOS | Keychain holds the AES-256-GCM file key | Native build/install, permission, lifecycle and signing/notarization qualification remains #62. |
| Android | App-owned Android Keystore AES-GCM adapter | Native bridge wired; authentication remains disabled pending #55. |
| iOS/iPadOS | Native Apple Keychain adapter | Authentication remains disabled pending #56. |

There is no plaintext credential fallback. **Retry protected storage** re-probes a locked store and restores valid saved authority disarmed. Corrupt state remains inspectable; **Forget saved pairing** explicitly deletes only this app's saved credential before allowing a new installation identity. Failed deletion preserves existing authority. Forgetting pairing and clearing public panels are separate actions.

Only the trusted local main window gets two narrow application commands (lifecycle/client request). Remote navigation is denied. The Android native storage plugin grants no JavaScript commands. No JavaScript network/filesystem/shell/controller-driver permissions are added.

## Development and native builds

Run from the repository root with the pinned Rust toolchain, Node and pnpm:

```text
pnpm install --frozen-lockfile
pnpm companion:check
pnpm companion:test
pnpm companion:ui:dev
pnpm companion:dev
```

Browser preview uses `http://127.0.0.1:1431` and has no native pairing/remote input. Local preview records shared control transitions only; it is separate from offline cached viewing.

Build on each native desktop OS:

```text
python scripts/companion-package.py --print-plan
python scripts/companion-package.py --output-dir <evidence-directory>
python scripts/companion-package.py --verify-only <evidence-directory>/<platform>/development-evidence.json
```

The helper tests locked native dependencies, rejects host/driver dependencies, builds **unsigned development** NSIS (Windows), Debian (Linux), or zipped .app (macOS), and records sizes/SHA-256. It does not install, publish or qualify them. The desktop CI matrix runs the same commands and uploads artifacts. Native prerequisites follow [Tauri's distribution guide](https://v2.tauri.app/distribute/); Linux also needs DBus/Secret Service development headers.

Mobile SDK entry points:

```text
pnpm --filter @opensimgear/companion android:init
pnpm --filter @opensimgear/companion android:dev
pnpm --filter @opensimgear/companion android:build:dev
pnpm --filter @opensimgear/companion ios:init
pnpm --filter @opensimgear/companion ios:dev
pnpm --filter @opensimgear/companion ios:build:dev
```

Android needs the configured SDK/NDK/JDK; iOS needs macOS/Xcode and signing/provisioning. Generated projects are ignored and reconstructed from the checked-in Tauri configuration/native plugin. Debug APKs use Android development signing; this is not release qualification. Candidate floors are Android API29, iOS15, macOS11 and Windows10/11; Linux CI builds on Ubuntu24.04. These are configuration choices, not a verified support matrix.

Desktop/mobile physical install, credential/permission and lifecycle checks remain open in #55–#62. See [C3 verification](../../docs/verification/2026-10-16-companion-c3.md) and [the feature guide](../../docs/companion-uis.md).
