# Companion C0 implementation and feasibility — 16 October 2026

Issue #38 implements the chosen Tauri 2/Svelte 5/TypeScript foundation for #37,
including a separate Windows desktop companion target. Android API 29 and
iOS/iPadOS 15 are candidate build floors, not qualified platform support. The
contracts, ownership, budgets and production trust requirements are recorded in
[the companion plan](../companion-uis.md). Full designer rendering is C1; the
production listener/trust/client integration is C2/C3.

## Delivered boundary

- `packages/companion-contracts` owns strict portable document and wire validators,
  canonical publication hashing, normalized values/axis quantization, typed
  feedback provenance and a pure bounded session/ownership model. Versioned JSON
  examples include panel, hello, full baseline, input, heartbeat and feedback.
- `apps/companion` is a separate application/storage/permission boundary with a
  reference Svelte panel, local recorder and minimal Tauri lifecycle shell. Its
  Rust dependencies do not include Cockpit's gateway, mapping, virtual-controller
  or visibility crates. Native events invalidate preview; they do not enable
  input or silently resume after backgrounding.
- Workspace scripts and lockfiles build/typecheck/test the shared package and
  companion independently of the host. CI adds frontend contract/renderer checks
  and native Windows shell compilation/tests. Rust shell startup and mobile event
  branches are not claimed as measured native adapter coverage.

The reference panel is fixed-layout, with XY axes, two momentary buttons, a hat
and labelled local sample feedback. It is not an arbitrary-document renderer or
the visual designer. No LAN listener, pairing, credential store, asset downloader,
virtual report, game binding or local driver change is implemented/enabled by C0.
The desktop target exists, but running a panel connected to a host on another PC
still needs C2/C3; local recording is not cross-PC qualification.

## Local validation

Checks ran in a dedicated native Windows mirror containing the committed host
baseline plus only owned C0 files, with an isolated Rust target. Existing shared
working-tree edits to desktop preferences, game integrations and other host code
were excluded and preserved.

| Check | Result |
| --- | --- |
| Strict shared TypeScript and companion Svelte/typecheck | Pass; zero Svelte errors/warnings |
| Contract tests | 64 pass; 100% lines, 98.10% branches, 98.53% functions |
| Recorder lifecycle/pointer ownership tests | 4 pass; 100% lines/branches/functions |
| Companion production frontend build | Pass |
| Native Windows Rust tests, locked dependencies | Pass; one lifecycle-epoch test |
| Windows embedded-assets executable build | Pass, Rust 1.90; unsigned development artifact |
| Existing Cockpit frontend check, transport tests, eight coverage commands, UI build | Pass |
| Independent focused review | Replay, ownership, limits and documentation findings resolved |

Regression tests cover stale session/input/baseline/cancel/heartbeat rejection,
per-client generation and controller-instance ownership, explicit takeover,
full 128-control neutral baselines, cancellation/role changes before initial
arming, rate-history preservation across rearm, generation-bound native
acknowledgements, lease/queue/rate/counter exhaustion and peer isolation. Initial
review exposed replay/rearm and stale-ack issues; they were fixed and tests were
rerun. The pure model has no autonomous scheduler or authentication side effects;
C2 must supply those trusted boundaries and correlate/deduplicate native delivery
acknowledgements before changing queue counts.

The browser renderer was exercised through actual UI controls: Start enables local
preview, a primary button records ordered true/false, hat-up records 0 then -1,
and Stop returns to Disarmed with controls disabled and zero active pointers.
The disarmed responsive panel was visually inspected. This is browser/renderer
evidence, not physical multitouch or native suspend/lock qualification. Native
UI automation timed out awaiting app access, so no native interaction pass is
claimed. The native shell compiles and its pure epoch test passes; mobile target
builds and native lifecycle behavior remain unverified.

An offline lockfile-only install initially lacked cached optional registry
metadata. A normal lockfile update followed by frozen installation succeeded,
reusing all packages. The first native build needed an application icon and a
distinct library basename to avoid a Windows PDB collision; both were corrected.
Rebuilding while the temporary prototype was running hit its executable lock;
only that owned prototype was stopped, and the final locked tests and embedded
build passed. No running Cockpit instance was stopped for this work.

## Acceptance split

At the user's request, missing qualification checks are retained in Backlog:

| Follow-up | Missing evidence |
| --- | --- |
| [#55](https://github.com/opensimgear/cockpit/issues/55) | Real Android multitouch/lifecycle, LAN trust/permissions and signed install |
| [#56](https://github.com/opensimgear/cockpit/issues/56) | Real iOS/iPadOS lifecycle, LAN trust/permissions and signing/provisioning |
| [#57](https://github.com/opensimgear/cockpit/issues/57) | Another Windows PC, native focus/lock/sleep/network behavior and installation |
| [#58](https://github.com/opensimgear/cockpit/issues/58) | Packaged mixed peers, production trust/load and measured latency/freshness |

Candidate floors and a truthful matrix are documented now; actual supported-device
qualification and the original real-mobile-device criterion are transferred,
unchecked. The local executable/recording portion of the added desktop criterion
has evidence, while real second-PC operation remains explicitly separate.
Implementation acceptance does not pass a transferred check. #37 stays active;
#39/#40 are the next implementation milestones. All existing unqualified host
capabilities and optional feature defaults remain unchanged.
