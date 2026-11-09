# Companion C3 mobile application — 16 October 2026

The retained mobile app has a separate identity, narrow native API, denied remote navigation, verified four-panel/24 MiB public cache, guarded asynchronous sessions and explicit reconnect/Start behavior. Credentials and action queues never enter the public cache. Android uses an app-owned Keystore bridge; iOS uses Keychain. Authentication remains disabled pending #55/#56.

Desktop shells, installers, desktop lifecycle checks and their reproduction commands have been retired. Use the [browser companion](../browser-companion.md) on desktop and the [mobile instructions](../../apps/companion/README.md) for Android/iOS.

Historical shared app helper validation recorded 21 passing tests; the Android portable codec recorded one passing test. These are historical software results, not physical mobile qualification. The Android build job is retained in `companion-mobile.yml`; iOS requires macOS/Xcode and provisioning.

Reproduce shared checks with `pnpm companion:check`, `pnpm companion:test` and `pnpm companion:ui:build`. Native mobile builds require the respective SDKs. Device storage/trust, touch, suspend/resume, LAN permissions and signing remain #55/#56; mixed-peer load and display checks remain #58/#59. No host capability gate changes.

## Discovered short-code pairing endpoint — 9 November 2026

Cockpit advertises the configured pairing hostname or IP in DNS-SD TXT
alongside the listener port. The mobile form uses the exact endpoint
returned by the native client. Discovery entries require the host field
and carry no pairing authority. The short-code exchange still compares
the selected endpoint with the authenticated offer and verifies the
pinned CA and server name before presenting the pairing secret.

The native Windows transport test used multicast discovery to find a
private listener advertised as `LOCALHOST`. It verified the exact endpoint,
completed short-code pairing and tested presence, assignment, credential
reload and revocation. Unit tests exercised hostname and IPv6 formatting,
the 253-byte hostname bound and rejection of malformed hints. Reproduce
the multicast run with `OSG_TEST_PAIRING_DISCOVERY=1` and the
`short_code_pairing_persists_both_ends_tracks_presence_and_revocation` test.

All 243 Windows companion library tests passed. LLVM measured 92.36% line
and 80.00% function coverage for discovery, 87.66% and 73.13% for the
server, and 95.21% and 90.84% for the client. This instrumentation did
not collect branches. The mobile frontend passed 35 tests, its coverage
gate, Svelte diagnostics and the production frontend build. The local
test and coverage output is under `.research/reactive/discovery-pairing-endpoint/`.

The rebuilt native client probe also passed pinned pairing, presence
handoff, stalled TLS cancellation and Forget against the running Windows
dev Cockpit at `f993800a`, using its existing manual pairing path. The
fixture reported no cleanup errors. Its trace is
`.research/reactive/companion-client-io-native/1794214946358/evidence.json`
with SHA-256 `daf2b8da9c11a93da1ea8dfd274b2f161b3d403b6353cf6e69e40f94dd33a340`.

Signed Android and iOS installation and actual device discovery remain
open.

## iOS discovery through Bonjour — 9 November 2026

The iOS client browses the declared `_osg-companion._tcp` service through
Bonjour. One connection waits for readiness and resolves the service on
its reported interface. A bounded set of operations tracks the current
services and drops a result when its service disappears. The TXT host
and SRV port produce the exact untrusted pairing endpoint. IP literals
are included as diagnostic addresses. A Local Network policy denial
reports where to grant permission before trying again.

Apple requires a restricted entitlement for raw multicast on iOS.
Browsing the declared service through Bonjour uses the app's Local
Network permission. The plist describes discovery and connection.
[Apple local network requirements](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)

The Windows companion suite passed 245 tests, including Bonjour TXT
length, key, endpoint and byte-order tests. The opt-in Windows multicast
short-code pairing test also passed. LLVM measured 93.21% lines and
82.35% functions in shared discovery and 100% of both in the TXT decoder.
This instrumentation did not collect branches. The Bonjour adapter and
shared code passed a type check against the installed Apple Silicon Darwin
Rust target in an isolated harness. Artifacts are under
`.research/reactive/ios-bonjour/`.

The rebuilt Windows client also passed the live dev Cockpit manual
pairing, presence, TLS cancellation and Forget regression on `f993800a`.
The trace is `.research/reactive/companion-client-io-native/1794215466772/evidence.json`,
SHA-256 `882b9e4374e2038e3633b04a374a7c4e170c741bdc4cf5965fab0a729a8db4aa`.

Linking the iOS app and checking Local Network denial and regrant,
discovery, and pairing on a signed device remain in
[#56](https://github.com/opensimgear/cockpit/issues/56) and the operator
follow-up [#199](https://github.com/opensimgear/cockpit/issues/199).

## Android Wi-Fi discovery lease — 9 November 2026

The Android shell acquires a Wi-Fi multicast lock for an explicit
nearby-host request, with the manifest's normal multicast permission.
Rust releases the matching lock on completion. Activity pause or
destruction releases it early, and a four-second one-shot deadline
retires an abandoned bridge call. A second request cannot acquire the
lock while the first holds it. A late finish cannot release another
request's lock.
[Android multicast lock reference](https://developer.android.com/reference/android/net/wifi/WifiManager.MulticastLock)

The Android dependency graph resolved from the locked manifests and the
portable Rust platform codec test passed with full coverage of its
Windows-compiled code. The manifest parsed and contains the multicast
permission. The packaging helpers passed 11 of 12 tests on Windows.
The simulator tarball test needs POSIX executable mode bits. The native
Windows multicast and short-code pairing fixture passed.

Android SDK/JDK compilation, signed-device Wi-Fi discovery and lock
release on pause remain in [#55](https://github.com/opensimgear/cockpit/issues/55).

## Android Local Network permission — 9 November 2026

The current Android template targets API 37. On Android 17 and later,
the app requests Local Network access when the operator selects
discovery, pairing, Connect or assignment refresh. A permission grant
asks the operator to select the action again so a request started
before the system dialog cannot resume later.
[Tauri Android template](https://v2.tauri.app/blog/tauri-2.12/)
[Android Local Network requirements](https://developer.android.com/privacy-and-security/local-network-permission)

Startup and Activity resume observe permission silently. The native
client parks passive presence and rejects new LAN work while access is
denied. Revocation retires existing I/O scopes and disarms the session.
A later grant wakes passive presence and requires fresh I/O scopes.
Status, offline cache and Forget remain available.

All 246 Windows companion tests passed, including the new denial,
revocation and regrant regression. The opt-in multicast pairing test
and five mobile build helper tests passed. The locked Android dependency
graph resolved and the permission declaration parsed. LLVM measured
95.30% lines and 90.44% functions for the client and 92.42% lines and
100% functions for presence. This build did not instrument branches.
Local outputs are under `.research/reactive/android-local-network/`.

The rebuilt Windows probe passed pairing, presence, TLS cancellation
and Forget against the running dev Cockpit at `f993800a`. The trace is
`.research/reactive/companion-client-io-native/1794216279235/evidence.json`,
SHA-256 `0448e9a8f46eda6d90b2cbcdf40e2ced786d669a2495a4ecaa9aa1348958650b`.

Android compilation, the system dialog, denial, regrant and signed
device behavior remain in [#55](https://github.com/opensimgear/cockpit/issues/55)
and [#199](https://github.com/opensimgear/cockpit/issues/199).

## Linux transport and POSIX packaging — 9 November 2026

All 12 mobile packaging helper tests passed under Ubuntu WSL2, including
the simulator archive's POSIX executable permissions check. Android SDK
and Xcode builds and signed mobile shell qualification remain in #55 and
#56. This run exercised the packaging helpers and transport library.

The Linux suite used an isolated D-Bus session and temporary Secret Service
store. The original backpressure regression failed because TCP accepted
one byte after the earlier write timed out. The fixture now continues
writing until it reaches backpressure again and then checks cancellation.
The socket implementation is unchanged. The locked Linux suite passed
232 tests with one explicit ignore. The locked Windows suite passed 246
tests. LLVM measured 94.37% line coverage and 100% function coverage in
`server/socket_io.rs` on both platforms. These runs measured regions, lines
and functions.

The opt-in multicast discovery, short-code pairing, presence and revocation
test passed on Windows. It also passed as a separate Ubuntu probe with the
fixture's Windows-only build guard removed in the temporary source copy.

The Windows native probe passed pairing, presence handoff, blocked TLS
cancellation and Forget against the running dev Cockpit built at `ec7348da`.
The trace is `.research/reactive/companion-client-io-native/1794220887352/evidence.json`,
SHA-256 `195df8cdeb1be55670d869a2bdafb754b39a6c96029347548f85347b0eb78d25`.
Local transport and coverage outputs are under
`.research/c193-reconnect/linux-companion-local/`.
## Pending pairing across native lifecycle changes

The standalone Companion now invokes the client's cancellation path on a
native lifecycle event when an operation is active. This includes an
in-flight pairing or assignment refresh. Retirement clears the pending
request ID and invalidates the operation's I/O epoch. Idle resume preserves
the current epoch. The real TLS regression waits for host approval,
suspends the client, then has the host approve the old request. The client
retires in under one second with no credential write.

Against the running Windows dev Cockpit from `c8a57cb0`, a candidate
service-library client paired with its private listener and performed three
presence handoffs. The same client cancellation method stopped a subsequent
TLS-stalled pairing attempt in 56.38 ms, advancing epoch 5 to 6. The client
remained disconnected from the cancelled attempt, restored its saved
host's passive presence, then disconnected on Forget. Cleanup passed.

All 246 Windows companion service tests passed. The changed client reached
95.12% production line coverage and 90.51% LLVM function coverage. LLVM
emitted no branch counters. All 41 packaging and release-check tests passed
under WSL. The Windows run passed 40 and failed the POSIX simulator bundle
execute-bit assertion. The live Windows probe exercises the shared native
client. Signed Android and iOS builds and OS-driven lifecycle tests remain
in [#55](https://github.com/opensimgear/cockpit/issues/55), [#56](https://github.com/opensimgear/cockpit/issues/56), and [#199](https://github.com/opensimgear/cockpit/issues/199).

| Local artifact | SHA-256 |
| --- | --- |
| `lifecycle_probe.exe` | `c827b551ff8361e5a3074b185813c663d4cae2ed21e3883cbbb236b5bf99a256` |
| Instrumented Windows companion test binary | `12b5ae14c332f27441412d811d053eba5ddd975b9bfe05d8aebb2863ef9b5308` |
| `companion-client-io-native/1794230565574/evidence.json` | `3810c855ce75f2b43addcf0f4eee6a545a9780d6408bf7461b295f5fb9a26542` |

### Requests queued before a lifecycle change

Network actions now carry the renderer's observed lifecycle epoch. The
native shell checks it before requesting Android permission, and the client
checks it before network work, after the protected-store probe, and after
claiming the worker slot. The worker also reserves the original client epoch.
Android applies permission results under the existing serialized update
guard, so a late result cannot overwrite a newer lifecycle's permission.
Status, neutralization and Forget remain available after a lifecycle change.

The Windows service regressions suspended a request while the protected
store was blocked and retired another between dispatch and worker admission.
Neither created a new worker epoch. The permission test rejected a stale
denial and kept the current I/O scope live. In the running Windows dev Cockpit
from `c8a57cb0`, the candidate client rejected a stale request and a fresh
action paired with the same offer. Presence handoff, TLS cancellation and
Forget passed with no cleanup errors. This probe called the shared client
guard directly. The signed mobile shells and OS callbacks still require
device qualification.

The final Windows service suite passed 248 tests. Client coverage reached
95.22% production lines and 90.65% LLVM functions. The companion frontend
passed all 35 tests, its coverage gate, Svelte diagnostics and a production
build using the installed tools.

| Local artifact | SHA-256 |
| --- | --- |
| Candidate `lifecycle_probe.exe` | `7b85d07d7847e355a817a6c49fe1bc8d05cd9fc1977e7e267975826165049991` |
| Instrumented Windows companion test binary | `6cbe141d1fb00c290c9ed7b1a1190536918d40fd0a44a13b7272f1e83362a04a` |
| `companion-client-io-native/1794231227027/evidence.json` | `d108a3a78db75358bf21b9bc0bf6c587fcc3cc5b506f97fb46903be1069e71b5` |
