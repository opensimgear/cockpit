# Native profile game launch

`profile_game_launch {gameId}` uses the current native installation and saved
Manual/Automatic selection policy. The request cannot supply a profile, process,
owner, executable, Stage, generation or launch argument.

The Manager stages a hardware-only saved candidate without holding admission or
the coordinator. Final admission checks the exact saved graph, local pins,
policy, installation, priority and cancellation epoch. It creates a journaled
neutral hold through Visibility, advances and binds its exact epoch under one
coordinator guard, then admits one native spawn. There is no reducer, script,
timer or graph output while awaiting the process. Process waiting and optional
provider preparation happen outside admission; Stop and status remain usable.
The first atomic request-arrival epoch is carried through native dispatch
outside JSON. Waiting for admission cannot adopt a later Stop epoch: both Apply
and launch reject that stale arrival before Manual pinning or host preparation.

The background watcher must confirm the exact PID, executable and creation epoch
returned by native startup observation. A replacement cannot satisfy this wait,
and UI polling cannot advance watcher debounce. An externally started process
found after preflight prevents a second spawn. Final Start consumes a fresh
native game-context Stage, preserves the hardware owner and original restoration
receipts, and adopts the actual watcher selection before automatic polling resumes.

Manual launch may explicitly resume a previous Stop only after fresh admission;
a queued Stop invalidates that attempt. Automatic inhibition requires explicit
Automatic resume. Failed read-only preflight preserves the prior healthy owner.
After ownership acquisition, errors cancel only that exact hold and inhibit
automatic retry. An uncertain spawn is not replayed and the process is not killed.

Optional provider setup is separate from process identity. A hardware-only
profile can start with its integration branches dormant. Automatic with no
matching rule uses Default with no selected context. A higher-priority verified
running game may remain selected while another game is launched. A corrupt
optional automatic-settings file remains preserved and blocks Automatic; the
existing explicit in-memory Manual/Default exception still works. A read-only
profile library is always rejected.

Successful replies contain normal `state.profiles` plus
`launch {state:"started", gameId, profileId, selectedGameId}`. This bounded result
is captured under final native admission after Start. It exposes no native
authority. General attachment to an already-running profile is outside this API.

Focused native tests cover nine actual Manager request flows: Manual startup
without optional integration; Automatic matching rule; Automatic Default without
context; higher-priority other game; Stop during process wait; Stop/status during
blocked preflight; prior Stop and retained optional-policy corruption; uncertain
spawn cleanup without process termination/retry; and failed preflight preserving
the previous owner. The strict Manager boundary also rejects unknown authority
fields and missing/unsupported game identities.
Two additional boundary tests complete a real Stop between the captured arrival
and native dispatch for both Apply and launch, and reject every supplied JSON
epoch alias. They verify zero host preparations/spawns, unchanged post-Stop
policy, retained inhibition and no new owner. The prior-Stop-before-arrival
Manual launch case remains valid.

Process-proof evidence is in `.research/profile-launch-process-coverage`:
five watcher and thirteen launch-helper tests passed; source-stamped production
coverage was 99.37% for `game_activation`, 94.19% for its runtime, and 95.77% for
`profile_game_launch`, against the unchanged 71% per-module gate. Manager
orchestration coverage is measured separately with the composed native boundary.

The final source-stamped gate is retained in
`.research/profile-launch-arrival-final-coverage`: all 11 Manager launch flows,
12 Automatic tests, the strict authority boundary and the existing slow
Stop/Pause/recovery, competing Apply/saved-edit and saved-revision/navigation
regressions pass. Production coverage is 396/438 lines (90.41%) for Manager
launch orchestration and 495/520 (95.19%) for Automatic, against the unchanged
71% per-module threshold. Slow preparation checks its original native arrival
both before and after the work, so a completed newer control takes precedence
over a late preparation error as well as a late successful Stage.
