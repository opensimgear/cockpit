# Panel events and captured game mappings

Panel events are an explicit input source in Mappings. Choose the named panel-event route, inspect its controls, save the local assignment and start a recording preview. This route reads the ordered, nonconsuming device event log, including inputs whose gateway HID is disabled. It does not use HID snapshots. Buttons retain event order; axes use the latest available calibrated state. The protocol has no panel-event hats. Source inspection and preview do not disable HID or change hardware settings.

Each subscription owns its cursor and checks gateway identity, route epoch, node, lease, module boot and event stream. A gap or reboot releases dependent contributions and requires a complete new baseline; script state and timers remain invalidated until explicit restart. Queues are bounded. Loss of a panel worker invalidates its panel sources while independent host-controller/synthetic sources keep running. Duplicate routes to the same module and known combinations of a panel's HID and event paths are rejected. Ambiguous OpenSimGear HID identities are rejected conservatively.

## Game profile migration

Games → choose a profile → Load saved mappings → choose a saved mapping → Capture mapping and upgrade profile. Capture explicitly upgrades that profile to schema 2 and stores a copy of the mapping, script source, assignments, target layouts and optional visibility intent. Save retains the snapshot. Neither capture nor save starts a mapping. Older schema-1 profiles remain readable and are not rewritten merely by loading them; schema-1 recovery journals still recover. The enclosing saved library remains schema 1.

Virtual output and hiding are independent requirements. A profile requesting neither can use recording mapping without either dependency. A requested unavailable operation fails preflight before peripheral writes or launch. Current game sessions reject virtual-output and visibility requirements because these paths are not qualified. No automatic fallback substitutes broader hiding or silently ignores a required capability. Manual preview remains available on every supported desktop platform; native DCS/Rally coordination remains Windows-specific.

## Ownership and recovery

Before hardware changes, the game journal stores desired mapping intent, prior active profile/assignments/token, local settings and ownership phases. The coordinator stops the prior mapping, applies peripheral configuration, waits for layout readback, then obtains fresh inputs and arms the captured recording mapping. Existing integration sequencing and process identity checks are preserved. Later edits to stored mapping documents do not change either the captured run or the prior snapshot used for restoration.

On failure or deactivation, mapping output is neutralized before hardware restoration. The previous recording mapping resumes with new baselines and empty state/timers during ordinary cleanup. Emergency mapping Stop suppresses this resume. After a process restart, explicit recovery keeps both captured and prior mappings stopped; no macros or scripts are replayed and no hiding is reapplied from a journal. Recovery preserves external run/settings changes and reports a conflict. Explicitly keeping the current setup archives the journal and releases only the session's ownership metadata.

Status, discovery, trace inspection, mapping Stop and Stop hiding remain available during a game session. Other mapping/controller mutations are guarded. Known shared panel controls between mapping and direct simulator commands are rejected with an overlap diagnostic. Arbitrary game bindings are not inspected. Existing DCS-BIOS direct-command routes still require disabled direct/gateway HID; virtual mapping eligibility does not relax that rule.

See [M6 verification](verification/2026-10-15-panel-game-m6.md). Virtual output preparation plus qualified hiding/capture verification at the game launch boundary, physical game rollback and device latency remain qualification work. Do not enable these paths merely because mock/recording tests pass.
