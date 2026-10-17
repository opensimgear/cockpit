# Hardware-first game launch helper

This native-only unit prepares a unified profile launch without using the legacy game hardware profile, mutating a graph, or applying a profile twice. It is not connected to Manager IPC or the launch button yet. It does not establish live-output or HidHide qualification.

`GamesRuntime::prepare_profile_launch` requires a writable `profile_session::Candidate` with an actual native `Stage`, no selected game context, a native cancellation epoch, and game eligibility. It captures the current validated installation and rejects an already running game. Compiler and Host preparation already support the hardware-only candidate: game resources remain dormant until an actual matching context exists.

After the hardware Apply succeeds, `arm_profile_launch` binds the immutable candidate proof to the exact active hardware owner and its committed admission epoch. Orchestration must supply the epoch returned by the commit's atomic `fetch_add` plus one. Loading a later current epoch could adopt an intervening Stop or operator change. Arming does not change the Stage fingerprint or replay its consumed payload.

`spawn_profile_launch` consumes the armed ticket. Its caller holds shared native admission only around the callback and supplies a fresh native candidate, status, epoch and operator latches. The helper requires the same running owner, revision, semantic snapshot and installation. The short `Platform::spawn_process` operation only creates the process; its default implementation explicitly rejects unsupported platforms. The old blocking `Platform::launch` remains for legacy paths until retirement.

`wait_profile_launch` runs outside admission and coordinator locks, observes the native process for at most 45 seconds, and checks exact owner/epoch cancellation both before and after each process read. It returns process metadata, not a verified provider context. No launch is retried, including an uncertain spawn acknowledgement. Failure returns an exact owned cleanup disposition; Manager must compare current ownership before acting. This helper never stops a profile or kills a process.

Explicit hidden-resource and virtual-controller declarations produce startup enumeration notices. Such declarations do not prove whether a running game enumerated or accepted any device.

## Validation

- Locked native test build passed on Windows.
- All 10 `games::launch::tests` passed. These exercise the actual Candidate and Stage types with a deterministic native-platform fixture; they do not activate hardware.
- Focused native production coverage passed at 226/245 lines (92.24%), with unchanged source stamps. The default CI gate includes this module and all existing native module filters.
- Coverage includes consumption of the original Stage, post-commit epoch binding, same-profile owner replacement, changed candidate/installation, all operator latches, expired preparation, uncertain/repeated spawn, late Stop during process lookup, startup deadline, process validity and explicit enumeration notices.

## Remaining launch integration

Actual Manager admission must check fresh native hardware tokens and baselines in addition to running status, reserve the committed epoch atomically, and retain Stop priority. No Manager path is added in this unit.

Attaching the verified game must preserve the existing hardware owner, reducer, virtual session, physical/companion owners and visibility/override receipts. The compiler includes dormant game endpoints in the program already; a new game-context preparation can therefore require identical portable program/policies and unchanged non-game delivery pins. It must acquire only the new game reader, prove advancing baselines, and attach its native delivery/context under a child owner. The parent RunOwner must remain unchanged because hardware receipts compare it exactly. On failure or context loss, revoke that child and deliver loss before timer advancement while retaining unrelated hardware state. Full Coordinator Apply is not a substitute: its takeover neutralizes and releases the outgoing virtual session.
