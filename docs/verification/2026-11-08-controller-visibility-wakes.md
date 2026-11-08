# Windows controller visibility waits

The HidHide worker registers a report watch for the selected devices before
it mutates policy. After hiding, it records the current baseline and waits
for a strictly newer report from each selected controller. Report commits
and cancellation wake the worker directly. An unchanged control value can
still prove a new report. The controller session, generation and interface
must match, and the two-second phase deadline still bounds the wait.

After the fresh baseline, the report watch retires. Registry inventory
commits wake the active worker for source validation. Queued handoff reads
and Stop also wake it directly. The worker keeps its separate 250 ms
external HidHide policy, executable and device-instance audit.

## Local verification

All 170 Windows controller tests and 37 configured production line coverage
checks passed. The visibility worker reached 91.50%, registry inventory
97.03%, capture registration 100%, and the retained handoff reader 92.71%.
The regressions cover a report between reading and waiting, a repeated
value with a newer sequence, source loss after arming, watch contention,
freshness timeout, and cancellation with journal restoration.

The rebuilt native dev Cockpit passed the controller catalogue and delayed
preview tests against an attached VKB Gladiator. Two catalogue readers saw
the saved rename, and the reopened reader showed the restored name. A slow
preview held one delivery while native capture advanced from sequence 43
to 53, then resumed at 53 with zero errors.

Executable SHA-256
`fb69d5251ea21906baf9b8728f9c67368badf31068f7a2d0b1e9d0128ab44787`.
The live tests used a private Cockpit database and passive input. The driver
mutation and restoration scenarios used a fake driver and the real registry
in the Windows tests. Physical HidHide apply, release and latency remain
separate acceptance in #199.
