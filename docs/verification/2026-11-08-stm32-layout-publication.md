# STM32 automatic layout confirmation from firmware publication events

Current STM32 firmware publishes sequenced invalidations when its layout
generations, transition flags, dirty state, or layout error changes. While
the automatic layout is pending and that feed is live, Cockpit waits for the
next publication, stream loss, shutdown, or the original operation deadline.
Each change prompts a snapshot from the gateway. The snapshot is checked
against the exact publication record before dispatch and again at reply.

Cockpit samples the publication record before issuing a command or reading
the initial pending snapshot. A change received before a pending reply is
therefore already visible when recovery begins. A newer event during
confirmation invalidates the older reply. The next read confirms the target
on the new record without sending the lifecycle mutation again. Gateways
with unavailable publication evidence retain the bounded snapshot retry.

The first CAN heartbeat can make a route ready without a layout event.
Command route readiness and physical layout qualification remain open.

The rebuilt native Cockpit passed the live workspace and both USB view
checks in a private database. The workspace fixture observed its shared
baseline without frontend discovery or state polling. Three native serial
fixtures passed. They verify that a quiet feed does not prompt reads, an
event before the pending acknowledgement still prompts confirmation, and
a newer event fences stale preflight and confirmation replies. Each
fixture sends Disable once.

Dev executable SHA-256
`7e22f994ce8ea9499d573305b23c4688132cb5b4f9e1c4d669be1308a595be84`.

The Windows native suite passed 2,453 tests with 18 default ignores. Two
pinned DCS archive tests passed separately. All three changed production
modules exceeded the 71% line coverage threshold.

| Module | Production lines | LLVM functions |
| --- | ---: | ---: |
| STM32 gateway | 89.27% | 91.96% |
| Publication feed | 90.08% | 73.53% |
| Layout recovery | 97.22% | 90.91% |
