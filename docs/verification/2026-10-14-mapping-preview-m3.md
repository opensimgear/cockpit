# Mapping preview and vJoy foundation verification — 2026-10-14

This records an incremental implementation of Cockpit #25. It does **not** close
M3 or qualify live output, provisioning, hiding, game binding, or another OS.
The application exposes recording previews and read-only vJoy inspection. Its
live-output, Create and Recover gate stays closed.

## Actual native and frontend checks

- Windows 11 x64, Rust 1.90: 230 native library tests, one bounded service framing
  test and one real native-service integration test passed. A separate build
  target kept the running development application open.
- Ubuntu WSL, Rust 1.90: 240 native library tests, the framing test and actual
  native-service integration test passed.
- Native mapping host: nine tests passed on Windows and Linux, including ordered
  physical transitions, baseline/reconnect behavior, history loss, persistence,
  and Stop racing an unfinished Start.
- Virtual-controller core: 17 Windows / 15 Linux tests passed. Coverage includes
  complete reports, partial acquisition, fallback failures, stale tokens, ordered
  sequences, pipe stalls, lease expiry independent of stdin/stdout, real child
  process deadlines, and provisioning transaction fault injection.
- Frontend: 135 transport/helper tests passed; Svelte checking reported zero errors
  and warnings; the production bundle built. Mapping helper line, branch and
  function coverage each reached 100%.

Production line coverage on Windows was 99.17% router, 98.28% mapping service,
98.28% settings, 100% report encoding, 97.87% feeder, 95.29% worker transport,
93.46% supervisor, 95.88% provisioning transaction, 80% inspection and 80.33%
Windows SDK adapter. Every measured production module exceeds the 71% gate.
Controller coverage remains above the gate on both tested platforms. CI also
tests portable components on native macOS; this is not device qualification.

## Production-service acceptance

The checked-in scripts/mapping-acceptance.py starts the real native service with
a temporary configuration root. It uploads a profile in bounded chunks, saves
explicit assignments, starts a recording preview, and verifies ordered synthetic
press/release edges, Stop, stale-token rejection and restart retention without
automatic activation. Unsupported live commands are rejected.

The Windows run also used the user-approved VID 231D / PID 0200 source. It selected
a button, X axis (raw 0..4095), and nullable eight-way hat, then confirmed a complete
baseline and ready preview. This did not ask the user to move controls: it proves
baseline routing, not physical gesture or game-binding acceptance. Linux used
synthetic inputs; ordinary Linux physical capture remains gated.

Fresh production inspection found pinned vJoy 2.2.2.0 x64 and slot 1 FREE, with
eight buttons, sixteen axes (0..32767), no hats and no FFB. No slots were acquired,
virtual reports sent or driver layouts changed. Local evidence is retained under
the ignored .research/mapping-m3 directory.

## Windows development UI repair

The Windows Tauri session used a Z: path backed by WSL. Native file watching
failed and Vite generated an invalid UNC module URL, leaving the WebView blank.
The repaired session runs Vite and Tauri from a local Windows mirror, with source,
test and simulation fixtures synchronized from the authoritative WSL checkout.
Rust build outputs and Windows dependencies remain local to Windows.

A screenshot and read-only WebView inspection confirmed visible gateway/device
controls and the Mappings workspace with actual vJoy inspection results. The
native UI is served on 127.0.0.1:1420. Local launcher/sync scripts and the screenshot
are research artifacts, not a cross-platform packaging claim.

## Open implementation and qualification

The production elevated registry/refresh helper and durable provisioning guardian
still require implementation. The
transaction tests do not substitute for those components. Ambiguous ownership
between key creation and journal flush is preserved as a conflict, never deleted
by guessing.

After integration, qualify two persistent devices, real physical button/axis/hat
transitions, game bindings and restart retention, layout mismatches and external
owners, cancellation/refresh failures, partial acquisition, source loss,
supervisor/feeder death, lease timing under load, and total-process termination.
Existing M0/M1 inverse-HidHide and native Linux/macOS gaps remain open. Optional
integrations stay independent and off by default. Unqualified capabilities remain
disabled.

## Supervised output follow-up

The parent lifecycle is now connected behind the unchanged application gate.
Fixture qualification uses the real native capture/router/feeder reducers with
injected driver calls. It checks explicit enablement, exact saved layout revalidation,
ordered short edges, quiet renewal beyond 250 ms without UI polling, unavailable
slots, device-hiding requirement rejection, helper death, recovery conflicts and
disable persistence after cleanup failure. A live Start requires an explicit Stop
first; a rejected replacement preserves the current run.

The helper protocol adds fresh-token recovery that rechecks FREE ownership and
exact shape. Parent report consumption is bounded to 128 records and an 8 ms work
budget per turn, with 100 ms reply deadlines and a separate 3 s launch handshake.
These bounds are implementation limits, not measured real-driver latency claims.
Error acknowledgements follow attempted fallback/release; even shutdown output
drain is bounded to 50 ms. Lost initial Start acknowledgements remain ambiguous:
the parent reports inspection required instead of guessing acquired ownership.

The real-service acceptance also invokes the production private feeder entry
with an empty target set. Installed vJoy is inspected in that child, but the empty
set is rejected before acquisition. Missing drivers exit with dependency guidance.
No virtual reports or layout mutations are part of this check.

Whole-app crash recovery journaling and elevated provisioning remain open, as do
the physical-device, two-slot, game-binding and loaded timing qualifications above.
The application still refuses live Start/Create/Recover.

Follow-up validation passed on Windows and Linux: all native regressions (230 /
240 library tests plus framing and process integration), 13 host lifecycle tests,
23 / 21 virtual-controller tests, and the production-service acceptance. Windows
coverage reached 98.49% router, 98.60% service, 97.62% settings, 93.67% parent
session, 95.92% worker, 98.10% feeder and 76.32% private entry; every module still
exceeds 71%. No physical output or layout mutation was performed.

## Native provisioning storage follow-up

The Windows registry backend now reads bounded raw descriptor and ancillary
snapshots, preserves registry value types, and uses one registry transaction for
descriptor creation and durable ownership. Recovery removes a descriptor and
retires its ownership in the same transaction. Missing descriptors also retire
ownership, preventing an old operation from deleting a later external recreation.
Canonical operation IDs, immutable plans and committed-state checks reject stale
or mismatched journal updates. Existing keys are never adopted as newly owned.

This is storage code behind the unchanged feature gate. It is not yet connected
to the elevated launcher, protected journal-directory setup, driver refresh or
guardian. Ordinary startup does not construct this backend, change the registry,
or enable Create. Those implementation tasks and physical qualification remain
open in #25.

Windows tests use disposable UUID-named HKCU fixtures, never the vJoy driver keys.
This process's HKCU transaction attempts returned Windows error 6801 (resource
manager inactive); the tests record that limitation and verify failure without
descriptor changes. The read-only SYSTEM-hive transaction probe succeeded.
Neither result proves atomic driver provisioning. On a Windows runner with
working fixture transactions, the same tests additionally exercise atomic commit,
rollback, external-key collision and removal/recreation protection.

Final local checks: 32 Windows and 22 Linux virtual-controller tests passed.
Windows production-line coverage was 96.74% for registry access, 76.19% for native
provisioning storage and 96.62% for the portable transaction core. Every measured
module still passes the 71% gate. The Windows Tauri development app rebuilt and
read-only WebView inspection confirmed the visible gateway UI at port 1420.
## Protected provisioning journals

The explicit helper preparation path now creates a journal directory with
Administrators ownership/group, protected access rules granting System and
Administrators full access, and Users read access. Each operation record receives
the same explicit owner and access policy. Every existing journal must match the
expected policy before it is trusted or updated; unexpected permissions are
preserved and reported, never silently repaired. This code is still not invoked
by ordinary startup or an enabled Create action.

Disposable HKCU fixtures verify native security-descriptor conversion, exact
policy comparison, refusal of an existing incompatible key and unchanged contents
after refusal. The local token cannot assign the Administrators owner (Windows
error 1307); the test records that qualification gap and confirms no key was
created. An appropriately authorized test runner also checks creation and
transactional use of protected fixture journals. No machine journal or driver
configuration was created by these local tests.

All 36 Windows virtual-controller tests passed; journal-security production-line
coverage is 95.45%, and every measured module remains above 71%. The earlier
storage checkpoint's Windows CI exercised real fixture transaction commits,
rollback, collision rejection and ownership retirement, reaching 97.22% native
provisioning-storage coverage. These fixture results do not qualify elevated
production launch, driver refresh, guardian recovery, or physical/game behavior.
## Bounded driver-refresh foundation

The native backend factory now supplies fresh subprocess inspection and a
dedicated refresh callback without loading an SDK or mutating anything at
construction. Refresh resolves exactly one present root HID-class instance whose
service is vJoy and whose device node is started without a reported problem.
Unknown, multiple, inaccessible or mismatched devices are refused. A fresh
inspection requires every published slot to be FREE before refresh.

The callback uses the Windows system-directory PnP utility with one exact device
argument. It has no shell expansion, inherited input, or unbounded output pipes;
completion, failure, timeout and child reaping are bounded. Provisioning still
requires subsequent complete layout/OEM readback. Successful utility exit is not
treated as successful provisioning.

Read-only device discovery, malformed native-property fixtures, wrong/ambiguous
target rejection, busy-owner preflight and harmless Python child completion,
failure and timeout tests passed. No PnP restart command was executed in these
checks. The refresh module reached 91.41% local production-line coverage. Actual
elevated refresh, cancellation/guardian coordination and physical recovery remain
unqualified, and ordinary Create/live-output gates remain closed.
## Guardian process-identity foundation

Read-only Windows process tracking now uses a PID plus canonical decimal-string
creation time, verifies the executable and same-user token, and retains the
original kernel process handle. Reused or exited PIDs do not become the old
process; access failures remain explicit. Tests cover current-process identity,
stale creation times, unrelated/missing executables, invalid handles and actual
owned-child exit observation. Process tracking never terminates the watched app.

The combined virtual-controller suite passed 41 tests on Windows. The new
process module reached 91.03% production-line coverage, alongside 91.41% refresh
coverage. These are building blocks for the guardian; the elevated launcher,
cancellation coordination, guardian entry and durable whole-app output recovery
still require integration and qualification. No registry provisioning, PnP
refresh or live virtual output was performed during these checks.
## Refresh recovery fence, 15 October

Commit beee690 distinguishes confirmed refresh-child exit from an uncertain
termination or wait failure. Before launching refresh, the protected operation
journal flushes a pending marker. Only confirmed child exit clears it. An
uncertain result or helper death preserves that marker and blocks descriptor
creation/removal, journal advancement and another refresh. Recovery cannot race
an unconfirmed PnP child; the ambiguous operation requires explicit inspection.

Fault injection covers failed termination/waits, confirmed failure and uncertain
exit. Disposable registry fixtures verify the durable fence and corrupt-marker
refusal. All 44 Windows and 23 Linux virtual-controller tests passed at that
checkpoint. Windows refresh coverage was 93.23% and native provisioning storage
78.64%. No PnP restart or driver mutation was executed.

## Durable output recovery, 15 October

Live session construction now writes bounded local intent before acquisition,
records ownership only after the feeder acknowledges Start, and records release
intent before Stop or recovery. Atomic replacement, file synchronization and an
OS file lock protect the record; external edits are preserved as conflicts.
Ordinary startup only inspects existing records and never acquires or resets a
device. This uses Rust 1.90's standard file locking:
https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock

An explicit recovery request after restart can send fallback for a previously
confirmed run, subject to fresh FREE ownership and exact-layout checks in the
feeder. It never restarts mappings. Interrupted Start/release acknowledgements
remain ambiguous; a separate exact-run acknowledgement clears the record without
touching output. Both actions remain behind the unchanged qualification gate.
Mappings shows pending/corrupt recovery status while recording previews remain
available. A new live run cannot replace unresolved recovery intent.

Tests exercise real process exit without destructors, OS lock release, competing
sessions, corrupt/oversized records, external changes, lost Start acknowledgement,
Stop/recovery ordering, restart fallback and no-output ambiguous acknowledgement.
They also verify that ordinary startup creates no recovery files and cannot
activate the gated recovery actions. The Windows Tauri WebView remained visible
and responsive with the existing gateway/device workspace.

All 50 Windows and 30 Linux virtual-controller tests passed; Windows mapping-host
tests passed 15 checks (the earlier 14-check suite also passed on Linux), frontend
type checking passed, and all 145 transport tests passed. Full local Windows
desktop regression passed 288 library tests and the framing test; three existing
live external-driver/game checks were ignored. The local checkout also contained
unrelated simulator work, which is not included in this checkpoint.

Production-line coverage: output journal 97.83%, session 91.53%, router 95.26%,
service 98.12%; every measured module exceeds 71%. Actual driver/process-loss,
game-binding and loaded timing qualification remains open. Elevated provisioning
launch, cancellation/guardian integration and its physical qualification still
remain to be completed. Create/live output/recovery stay disabled.
## Explicit provisioning integration, 15 October

The private Windows entry now dispatches the protected provisioning transaction
after checking elevation, exact same-user parent identity and an operation-scoped
cancellation event. A machine-wide helper mutex serializes layout operations.
Read-only preflight precedes protected journal preparation, and another abandoned
operation blocks new changes. Cooperative cancellation is checked between bounded
steps and after refresh; rollback itself is never cancelled. A pending/uncertain
refresh remains fenced for explicit inspection.

The parent now runs provisioning asynchronously, including the Windows
authorization request. Cancellation is registered before elevation starts, so
cancelling while authorization is pending cannot silently become an authorized
layout change later. The helper monitors parent exit and a 60-second operation
deadline. The parent reports unconfirmed completion after its own bound instead
of killing a helper during registry/driver work. This transient helper owns
cleanup; no persistent system guardian is installed. Killing every process still
requires explicit recovery and never implies universal automatic cleanup.

Create/Recover/Cancel IPC and the UI are connected. Create requires explicit
virtual-device enablement and stopped mapping. Live output and game activation
are blocked while provisioning is active; cancellation remains available.
A bounded read-only machine-journal inventory exposes interrupted operations
after restart. Outcomes require protected journal readback plus observed helper
completion. Custom layouts and FFB provisioning remain unavailable; the narrow
provisioner still supports only the M0 default descriptor.

The native entry and launcher have their own false qualification gate, in
addition to the mapping-service gate. The real built service executable returned
exit 78 for a valid private provisioning request, before elevation or mutation.
Read-only WebView inspection confirmed the running Windows app's Mappings UI,
empty recovery inventory, and disabled Create/Start/Recover controls. No UAC
prompt, driver refresh, layout mutation, hiding or virtual output was performed.

All 61 Windows virtual-controller and 31 Linux virtual-controller tests passed,
along with 15 native mapping-host tests on each platform and frontend type checks.
The full local Windows desktop regression and framing test passed again; the
same three external game/driver checks remain intentionally ignored. Native
launcher tests use a harmless hidden Python child without elevation. Registry
tests remain disposable fixtures, not production driver qualification.

Windows production-line coverage: coordination 93.14%, launcher 79.43%,
provisioning entry 76.92%, asynchronous jobs 81.25%, provisioning storage 81.17%,
mapping service 87.50%. Every measured module exceeds 71%. Actual UAC behavior,
driver refresh/cancellation, killed-helper/total-termination recovery, physical
two-slot/button-axis-hat routing, game binding/restart retention and loaded
timing still require qualification before enabling these controls.

Windows API references used for the launcher and synchronization behavior:
https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw
https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexw

## Configurable layout implementation, 15 October

The creation form now captures an explicit layout (up to 16 selected axes, 128
buttons and four continuous POVs), initially eight conventional axes, 32 buttons
and one POV. A bounded canonical request carries it into the elevated helper.
The helper emits only the recognized non-FFB grammar from the pinned
[vJoy source](https://github.com/BrunnerInnovation/vJoy/blob/bd268d1fa5934a900a79e76e519d4aa804e0ee3d/apps/vJoyConf/vJoyConfig/vJoyConfig.cpp).
It preserves the implicit default slot when introducing the first explicit slot,
checks every published capability against its descriptor, and refuses unknown,
FFB or discrete-POV configurations. This extends implementation coverage, not
native hardware qualification. All production provisioning/live/recovery gates
remain false; no driver layout was changed during these tests.

An independent HID short-item reader verifies axis/button/hat bit positions and
the fixed 768-bit report payload across sparse and maximum layouts. The release
default descriptor retains its recorded SHA-256. Tests cover byte corruption,
truncation, capability disagreement, custom-layout rollback, legacy journals,
canonical helper arguments and attempts to claim pre-existing implicit slots.

Validation: 68 Windows virtual-controller tests and 36 Linux portable tests pass;
15 Windows mapping-host tests pass; pnpm check reports zero errors/warnings.
Windows instrumented production coverage is 100% for descriptor.rs, 96.03% for
provision.rs, 81.88% for windows_launcher.rs and 78.35% for
windows_provision_entry.rs. All measured virtual-controller and mapping-host
modules exceed the 71% gate. Native driver enumeration, multi-slot physical
button/axis/hat routing, actual game bindings, ownership contention and
process/driver failure qualification remain open in #25. Refusal to alter
unrecognized/FFB layouts remains a documented capability limit.

Rendering follow-up: the configurable layout fieldset was initially nested in the pending-recovery loop. It now renders in the ordinary creation section, including when no recovery exists. The Windows Tauri WebView confirms the complete starter form is visible and Create, Start and Recover remain disabled. pnpm check passes with zero errors/warnings.
