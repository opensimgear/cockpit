# DCS CommonData telemetry and explicit quantities — 17 October 2026

This software proof addresses the DCS altitude source boundary in [P5](https://github.com/opensimgear/cockpit/issues/85). It uses native catalogue capture, the production DCS-BIOS decoder, the typed game reader and injected process observations. It does not establish a running simulator, physical altitude measurement, device output or driver qualification. Actual UDP/Host/browser composition is a separate proof.

The selected aircraft remains the mission authority. A selected A-10C catalogue may expose CommonData telemetry only when its nonempty installed `AircraftAliases.json` entry explicitly includes both A-10C and CommonData. CommonData is never selected as an aircraft, and no arbitrary alias module is imported. Its output namespace is collision-free: `channel:common:CommonData:ALT_MSL_FT:0`. CommonData commands are never advertised, even if an installed document lists inputs. Legacy panel queries and command identities retain their aircraft-only scope.

The profile catalogue captures the selected aircraft, authorized CommonData, aliases and start/end metadata documents together. A digest of their complete parsed contents participates in the native catalogue revision, so an address/mask edit cannot hide behind an unchanged scalar port contract. Mission/counter metadata is constructed from those exact captured values. Native reader construction checks the captured revision again and never accepts a browser-supplied source module or address.

An active reader checks actual regular-file metadata at a bounded 250 ms interval; Acquire, Start and command admission force that metadata check. A full document digest is checked at a bounded two-second interval, including edits that preserve file length and restore timestamps. Construction forces the full check after source resolution. Changed documents revoke the existing reader and its delivery publication; they do not replace its addresses, rebind, replay commands or reinterpret quantities. Missing optional CommonData produces no common ports; a present malformed document, nonregular file or symbolic link fails closed.

## Reported quantity and availability

The [official v0.11.7 CommonData source](https://github.com/DCS-Skunkworks/dcs-bios/blob/30125bd4231f87769f46bacd6050f25e2d25d1f8/Scripts/DCS-BIOS/lib/modules/common_modules/CommonData.lua) defines `ALT_MSL_FT` as reported MSL feet. Its generated descriptor is integer 0..65535 at address 1076, mask 65535, shift 0. A-10C and A-10C_2 explicitly include CommonData in the official alias table.

The upstream getter computes `floor(selfData.LatLongAlt.Alt / 0.304)`. Cockpit preserves that reported integer; it does not silently substitute the exact SI foot conversion for the upstream calculation or describe the quantity as AGL. The visible Interpret raw quantity node declares reported MSL feet without changing the value. A subsequent visible ft-to-m conversion uses the declared unit.

Upstream retains its previous internal value when ownship export is disallowed or self data is missing. Neither this channel nor MetadataStart/MetadataEnd advertises a measurement-validity bit: aircraft identity and counter progress establish export-stream context, not a new physical measurement. The actual port label therefore includes **CommonData reported** and **measurement validity unavailable**. A zero received on the wire remains a reported zero; no missing value is invented from it. Before bytes arrive the value is unknown; frozen export, wrong aircraft, NONE and context loss disarm freshness through the existing native guards. Graph feedback remains local/host provenance and cannot claim Game confirmed.

## Validation

The instrumented copied native executable passes 24 DCS profile tests (including five new CommonData cases), 28 DCS-BIOS tests and 11 native game-reader tests. One explicit archive-dependent test is skipped in the ordinary filter and then run separately: it passes against the **complete official** CommonData, A-10C, alias and metadata catalogues. It verifies normalized read-only discovery and actual parsed 16-bit readback, beyond the factual altitude descriptor subset used by fast regressions.

The archive is 8,234,988 bytes with pinned SHA-256 `3272bc07210fa6c560f115cfb5dc8881accf7af5e7a5e218e2c46e1fd89e3292`. Reproduce the explicit smoke with:

```powershell
$env:OSG_DCS_SETUP_ARCHIVE = '<checksum-pinned DCS-BIOS_v0.11.7.zip>'
<copied native test executable> dcs_profile::common_tests::pinned_official_archive_commondata_normalization_and_native_guard --exact --ignored
```

Focused production coverage is 1005/1066 lines (94.28%) for `dcs_profile.rs`, 811/850 (95.41%) for `dcs_bios.rs`, and 505/557 (90.66%) for `profile_session_games.rs`. The existing DCS >70% and game-adapter 71% thresholds are unchanged. Tests cover missing versus malformed common data, unauthorized namespaces, regular-file guards, exact source revisions, address-only and alias edits after a live baseline, same-size/timestamp edits, actual decoder zero/missing/stale/wrong-aircraft behavior and permanent per-endpoint command withdrawal while unrelated commands and telemetry remain fresh.

Private evidence: `.research/p5-altitude-audit/final/{build-manifest.json,tests-manifest.json,coverage-summary.json,lines.txt,test-0.log,test-1.log,test-2.log,official-archive.log}`. All 372 recorded source/fixture hashes remain unchanged across this focused execution. Cargo's actual test artifact was copied to `native-tests.exe`; its SHA-256 is `bdf54065a000e04a8105eec0965a2650fb9cbb453b58a565ffea1a14f6233fec`.
