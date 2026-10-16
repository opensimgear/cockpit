# ACR telemetry source audit — 2026-10-14

## Subsequent live correction

The user subsequently ran ACR and confirmed driving. Read-only captures showed
advancing physics and graphics packet IDs, status 0, empty version strings,
nonempty car/track, and max RPM 0. The earlier software-only audit below missed
this semantic difference despite matching offsets. It must not be read as live
qualification. The production parser/setup now supports this sparse variant as
`acr-prefix-2`, with packet progression and required-channel checks. Explicit
pause/replay values still disarm; unavailable state detection is reported.

Both opt-in Windows tests passed against PID 41148: actual WindowsSource + Session
returned fresh native RPM 5156, gear 1 and speed 34.6244 km/h; the complete native
setup probe returned compatible=true, nativeVerified=true, status=receiving for
SHA-256 `42c359b0cd24774a86441710fdc7d7db3bc06b57b3ff30cb9ea5d56fb71448fb`.
The probe does not persist setup settings. The desktop development watcher also
rebuilt/restarted Cockpit with the corrected source. Added regression coverage
checks sparse status, stale packets, explicit pause/replay, missing car context,
and optional maximum RPM. The focused Linux runs passed (33 ACR-filtered and
8 telemetry-filtered tests, with one test shared between those filters).

Reproduce live Windows checks with OSG_ACR_LIVE_PID and OSG_ACR_LIVE_EXE set:

```text
cargo test --locked --lib selected_running_game_produces_fresh_native_telemetry -- --ignored --nocapture
cargo test --locked --lib running_game_passes_native_setup_probe -- --ignored --nocapture
```

## Original source-only conclusion

Retain read-only Windows named shared memory for local ACR telemetry. The implemented prefixes agree with the best public ACR reader sources inspected. No public official ACR-specific telemetry specification, SDK, binary-layout guarantee, or native UDP telemetry protocol was located. This is a documented search result, not proof that private/partner documentation does not exist. No live ACR process was available during this audit; source agreement and passing synthetic tests do not qualify a game build.

## Official documentation search

Searched ACR telemetry/shared memory/API/SDK terms across the official Assetto Corsa site, Supernova, 505 support, the official AC forum domain, and ACR Steam discussions. Inspected the official [ACR product page](https://assettocorsa.gg/assetto-corsa-rally/), [Supernova product page](https://supernovagamesstudio.it/en/assetto-corsa-rally-2/), and [0.6 release page](https://assettocorsa.gg/assetto-corsa-rally-ea-06-out-now/). These did not provide an ACR telemetry SDK or layout specification. Indexed publisher references to telemetry concerned backend analytics rather than an external racing-data API.

AC, ACC and EVO documentation is for those products. In particular, the original AC UDP interface and EVO SDK/shared-memory guide cannot establish Rally support. No recommendation by Supernova/505 to use a particular public ACR telemetry API was found.

## Reproducible implementation sources

These are primary sources for community implementations, not official ACR specifications:

- LuizZak reader, revision `8bb26990def38ffd0e4afe71e04e85e5ddeb3670`: [Physics.cs](https://github.com/LuizZak/AssettoCorsaRallyTelemetryReader/blob/8bb26990def38ffd0e4afe71e04e85e5ddeb3670/ACRallyMemReader/Data/Models/Physics.cs), [StaticInfo.cs](https://github.com/LuizZak/AssettoCorsaRallyTelemetryReader/blob/8bb26990def38ffd0e4afe71e04e85e5ddeb3670/ACRallyMemReader/Data/Models/StaticInfo.cs). Explicit sequential Pack=4 and Windows Unicode declarations.
- decnet100 recorder, revision `eaef0f29c6712a2e0dbc3bfd8246b84dcdbb9586`: [physics parser](https://github.com/decnet100/acr_telemetry/blob/eaef0f29c6712a2e0dbc3bfd8246b84dcdbb9586/vendor/acc_shared_memory_rs/src/parsers/physics_parser.rs), [static parser](https://github.com/decnet100/acr_telemetry/blob/eaef0f29c6712a2e0dbc3bfd8246b84dcdbb9586/vendor/acc_shared_memory_rs/src/parsers/statics_parser.rs), [graphics parser](https://github.com/decnet100/acr_telemetry/blob/eaef0f29c6712a2e0dbc3bfd8246b84dcdbb9586/vendor/acc_shared_memory_rs/src/parsers/graphics_parser.rs), [status enum](https://github.com/decnet100/acr_telemetry/blob/eaef0f29c6712a2e0dbc3bfd8246b84dcdbb9586/vendor/acc_shared_memory_rs/src/enums/acc_status.rs). Inspected only; no upstream code copied.
- Additional cross-check: albertowd reader, revision `0dd9992cde4d599d8f914300957eb16955b05804`: [ACR source](https://github.com/albertowd/live-telemetry-evo/blob/0dd9992cde4d599d8f914300957eb16955b05804/src/live_telemetry_evo/sources/acrally.py), [shared declarations](https://github.com/albertowd/live-telemetry-evo/blob/0dd9992cde4d599d8f914300957eb16955b05804/src/live_telemetry_evo/sources/acc.py), [gear formatter](https://github.com/albertowd/live-telemetry-evo/blob/0dd9992cde4d599d8f914300957eb16955b05804/src/live_telemetry_evo/widgets/engine_view.py). Its ACR source reports live probing, shares the ACC declarations, and documents late/zero static data. This is an upstream author's report, not an independently reproduced live capture. Readers share AC/ACC ancestry, so agreement is not three independent live validations.
- Microsoft [OpenFileMappingW](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-openfilemappingw) and [kernel object namespaces](https://learn.microsoft.com/en-us/windows/win32/termserv/kernel-object-namespaces) establish Windows API behavior, not ACR field semantics.

## Field-by-field comparison

| Implemented item | Source comparison | Result |
| --- | --- | --- |
| Local\\acpmf_physics / graphics / static | Additional ACR reader uses these exact session-local names | Match |
| Physics packet at 0, int32 bit pattern | C# declaration and Rust/Python readers | Match; runtime interprets progression as wrapping u32 |
| Raw gear at 16, int32 | C# and Rust/Python readers | Match |
| Normalized gear = raw minus 1 | Additional ACR reader passes raw gear to formatter: 0 reverse, 1 neutral, 2+ forward | Match; still needs live reverse/neutral/first check |
| RPM at 20, int32 | All inspected declarations | Match |
| Speed at 28, float32 km/h | All inspected declarations | Match |
| Graphics status at 4, int32 | Graphics parser and shared Python declaration | Match; enum 0 off, 1 replay, 2 live, 3 pause comes from ACC-compatible source |
| Static UTF-16 version strings at 0/30, 15 code units each | C# Unicode and Python Windows wchar declarations | Match |
| Static car/track at 68/134, 33 code units each | C# and Python declarations; Rust parser progression | Match |
| Static max RPM at 412, int32 | C#/Python declarations and explicit Rust parser padding | Match |
| Minimum prefixes 32/8/416 bytes | Last accessed field ends at each bound | Correct; not full mapping sizes |

Static offset derivation: two 30-byte strings + two 4-byte integers = car offset 68. Five 66-byte strings end at 398; Pack=4 adds two padding bytes, then sector count at 400, torque at 404, power at 408, maximum RPM at 412. No host-native wchar width is used in our parser.

## Acquisition and validity review

`src-tauri/src/acr_windows.rs` opens existing mappings with access 4 (FILE_MAP_READ), maps only required prefixes, uses volatile byte loads, and releases both views and handles. It does not create or write game mappings. Local names require Cockpit and game to run in the same Windows session. Process access is limited query plus synchronization, not arbitrary process-memory reading.

The selected process basename and creation epoch are checked; process lifetime is checked around copying; known competing Assetto executables are checked at attach and periodically. Named mappings are not PID-scoped: these checks reduce ambiguity but do not prove mapping ownership, and the competitor list is finite.

Repeated equal copies are bounded best-effort checks only. No producer seqlock, memory-order publication contract, or atomic transaction across physics/graphics/static was found. Matching copies cannot prove that all pages describe the same instant.

`game_telemetry.rs` requires an advancing original physics packet after a baseline, handles unsigned wrap, and clears/disarms on stale, reset, context/source changes and non-live status. Invalid numeric fields become individually unavailable. Invalid maximum RPM disables max_rpm/rev_ratio instead of inventing a redline. These are sensible consumer policies, not official ACR guarantees.

## Remaining compatibility limits

1. The additional ACR reader reports zero/lazily populated static data, sometimes blank car identity even with numeric specs. Our setup verifier requires nonempty versions and car/track in live status. It can therefore reject otherwise usable RPM/gear/speed on a build that leaves metadata blank. It retries when data arrives, and normal acquisition re-reads static every poll. Keep this restriction explicit until a live capture establishes a reliable alternative identity; do not relax qualification based solely on community reports.
2. Our maximum RPM comes only from static offset 412. The additional reader uses physics currentMaxRpm as int32; the older C# declaration calls that later field float. We do not read that disputed extension. Missing static maximum can therefore leave rev-ratio unavailable even if a later physics field is usable. This is a deliberate compatibility limit, not an offset error.
3. Read-only open and correct offsets do not validate ACR's pause/replay status behavior. Verify those transitions and delayed static population on the exact game executable.
4. The setup probe establishes plausible native-compatible values and freshness, not calibration against dashboard readings. Version strings are recorded context, not an official schema allowlist.

## Validation performed

From Cockpit in Ubuntu/WSL:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib game_telemetry:: -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib acr_ -- --nocapture
```

Result: 7 parser/session tests and 31 ACR acquisition/setup/integration tests passed; 0 failed. Tests use authored fixtures and mocks. Windows-specific FFI acquisition was reviewed but was not executed by these Linux tests. Three unrelated existing hid_names warnings were emitted. No ACR process was running during the audit. No implementation changes were needed to reconcile the supported prefix with the sources.

## Live qualification still required

Record executable fingerprint, game/shared-memory versions and raw prefixes; compare RPM, speed and reverse/neutral/first/higher gears with the dashboard. Exercise initial menu, stage entry, delayed static metadata, pause/resume, replay, car/stage changes, restart and competing AC/ACC processes. Verify frozen packets time out, reattachment establishes a new baseline, and missing max RPM leaves rev-ratio unavailable. Preserve captures with their exact-build provenance before claiming live compatibility.
