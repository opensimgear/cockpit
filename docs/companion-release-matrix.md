# Companion release matrix

C6 prepares packages, examples, guides and release evidence. **No companion target
is release-qualified yet.** The implementation milestones C0-C6 do not complete
physical qualification or close [feature #37](https://github.com/opensimgear/cockpit/issues/37).
Desktop companions use the Cockpit-hosted browser UI. Android CI produces a debug APK. iOS requires a native Xcode build and provisioning.

## Client targets

| Client | Development artifact / candidate floor | Missing qualification |
| --- | --- | --- |
| Browser | Host-served HTTPS UI / optional PWA | Physical browser/LAN, display and performance qualification #58/#59; cross-version trust #68 |
| Android | Debug APK; API 29 candidate floor | Native store/trust, touch/lifecycle and signed distribution #55; display #59; upgrades #68 |
| iOS/iPadOS | Candidate shell; iOS 15 candidate floor | Xcode/device build, Keychain/LAN/touch/lifecycle/signing #56; display #59; upgrades #68 |

A build floor is a configuration choice, not evidence that every device above it
works. Mobile authentication remains disabled. Browser pairing credentials live in the dedicated HTTPS origin storage. No plaintext fallback is
supported. A development package or metadata match does not qualify installation,
trust migration or application upgrades.

The companion is a narrow client and needs no local vJoy/HidHide. Windows is the
selected **host** qualification target. Browser and mobile clients connect to the same host; they do not enable Linux/macOS host virtual output. Those backends
remain deferred in #48/#49. Windows host output and production recovery remain
#54/#66; existing unqualified provisioning, hiding and live scripts stay disabled.

## Evidence and release decision

[The machine-readable matrix](companion-release-matrix.json) preserves the browser and both mobile
candidate client targets, pending qualification stories and the C0 budgets.

```text
python scripts/companion-release-check.py
python scripts/companion-release-check.py --output .research/companion-c6/release-readiness.json
python scripts/companion-release-check.py --require-release
```

The ordinary check succeeds when the declared **blocked** matrix and disabled
native source gates agree. `--require-release` returns exit code **2** while
qualification is missing; malformed/inconsistent evidence returns **1**. The
check is a repository consistency guard, not a signing verifier, runtime probe or
automatic release authorizer. Editing the matrix cannot qualify a capability.
Native runtime tests and their own gates remain authoritative.

Mobile signed distribution, host/client upgrades, trust rotation, interrupted install and rollback remain [#68](https://github.com/opensimgear/cockpit/issues/68), reusing #55/#56 device evidence. Browser setup and trust are described in the [browser guide](browser-companion.md).

| Remaining evidence | Owner |
| --- | --- |
| Real mixed clients, shared endpoint, trust/fault/load and measured latency | #58 |
| Physical designer/native document workflows and display ergonomics | #59 |
| Real mapped virtual devices, game bindings, nonzero fallback and whole-process loss | #66, with selected host gate #54 |
| Real typed simulator feedback, concurrent gateway traffic and captured game assignments | #67 |
| Signed native installation/update and cross-version trust/rollback matrix | #68, reusing #55/#56 |

Use exact OS/architecture, app/host source and artifact hashes, driver/helper/game
versions, UI/publication identities, network conditions, timestamp boundaries and
observer logs. Record expected and observed results, including failures. A fixture
or source-accepted acknowledgment is not successful virtual submission, a game
observation or a native lifecycle pass. Keep those evidence classes separate.

## Limits and performance targets

At most eight active companions share 16 mapping source roles, eight targets and
256 nodes with other mappings. This is a ceiling; preflight may reject shapes or
reservations that do not fit. Each UI allows 16 pages, 128 logical inputs and
64 feedback channels. Wire input frames are at most 16 KiB / 64 changes, at most
120 batches/s/client, with 512 queued transitions. Feedback is bounded at 30 Hz;
heartbeat is 250 ms and the host lease is 1500 ms. Feedback becomes stale after
1000 ms. Asset/document/cache limits are in the [user guide](companion-guide.md).

Release targets remain **unmeasured**: touch/pointer to acknowledged successful
virtual submission p95 <=25 ms, p99 <=50 ms; feedback <=250 ms. #58 must measure
client/render, LAN, host, feeder acknowledgment and feedback stages under the
supported load and record queue/coalescing/missed-edge behavior. Loopback fixture
timing cannot qualify a LAN performance claim.

## User and operator documentation

- [Setup, designer, pairing, feedback, revocation and recovery guide](companion-guide.md).
- [Importable flight/button-box and axis/racing examples](../examples/companion/README.md).
- [Native companion development/build and protected-store behavior](../apps/companion/README.md).
- [C6 validation record](verification/2026-10-16-companion-c6.md).

There is no signed release publishing or automatic update path enabled by C6.
Qualify each claimed matrix row and capability in its own follow-up and reviewed
enablement change before advertising support or closing #37.