# In-review implementation audit — 15 October 2026

This audit distinguishes missing application paths from physical qualification.
It does not enable any capability or replace the original acceptance criteria.
Issues close only when their complete scope and acceptance criteria are met.

| Dependency order | Implementation finding | Remaining manual interaction |
| --- | --- | --- |
| #21 M0 | Prototypes and bounded contracts exist; no additional prototype implementation gap identified. | Existing custom/FFB layouts, inverse HidHide, setup/reboot, native Linux/macOS and contract approval. |
| #22 M1 | Production Raw Input/HidP registry and monitor are wired. Interactive foreground-game capture, reconnect and a port change are recorded in 71f5f3a. | Identical-device identity matrix and signed-axis hardware/live monitor qualification; the connected left/right Gladiators have different product IDs and unsigned axes. |
| #23 M1P | evdev/IOHID capture and permission/baseline paths exist behind qualification gates. | Physical native Linux/macOS devices and permissions/game matrix. |
| #25 M3 | Provisioning, descriptor generation, feeder, leases and recovery are implemented. | Two production-created slots, real controls/game bindings, Gremlin contention, UAC/failure/restart and whole-process termination. |
| #26 M3H | Policy validation, session ownership and journaled restoration are implemented. | Normal/inverse process/API scope, hardware dependency matrix, live conflicts and recovery. |
| #27 M3L | Native uinput creation/report/destruction and persistent templates are implemented. | Actual devices, permissions, recreation and native/Proton binding retention. |
| #28 M3M | Native helper IPC, HID descriptor/report path and packaging candidate exist. | Apple-authorized signing/entitlement, protected deployment, devices/games and lifetime matrix. |
| #29 M3V | Linux isolated launch was completed through 689ba6c; suppression adapters also exist. | Physical scope, alternative paths, device-number reuse, reconnect and native macOS seizure. |
| #30 M4 | Editor and transformations are implemented. | Real native output while the UI is busy; recording evidence is insufficient. |
| #31 M5 | **Fixed in a7fdd4b:** live startup carries compiled workers into the feeder router; supervision detects idle helper death without another callback. Production qualification gates remain false. | Physical scripted output, either-source unplug, loaded capture/feeder response and packaged resource limits. |
| #32 M6 | **Fixed in aeb6e53:** captured game sessions prepare neutral output, apply owned visibility, arm fresh baselines and roll back in ownership order. Prior recording resumes never gain optional effects. Qualification gates remain false. | Real DCS/racing, combined visibility/output rollback, reconnect/recovery and physical panel coexistence. |
| #33 M7 | **Fixed in this change:** native startup/preview/restart acceptance runs inside a private Linux device namespace with uinput absent; stopped-inspector serialization preserves its empty state. Board wording and evidence are reconciled. | Deployment/device/game matrix and physical capture-to-successful-submit latency. |

Implement #31 before #32, then complete #33's missing automated evidence. Keep
earlier qualification-only issues In review while arranging manual interaction.
Only the actively implemented issue moves to In progress. Final commits, CI runs
and checkbox evidence are maintained on the corresponding GitHub issues.
