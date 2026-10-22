# Native automatic profile selection

The portable policy accepts only native verified game observations and saved rules. A rule contains one registered game, its validated installation executable identity, a saved unified profile, an enabled flag and an explicit signed priority. PIDs and creation epochs are transient native observations, never saved rule authority.

Custom games and imported library entries use the same policy without requiring
a telemetry provider. Register the actual runtime executable in Games, then set
the profile's **Game context**. External startup and Cockpit launch both require
the same exact native process evidence. A Steam launch target is separate from
the executable being monitored. An unresolved installation cannot supply that
evidence, and a launcher timeout never counts as a successful game startup.

Stable custom IDs survive rename, relocation and repeated imports. Persisted
rules and profiles remain readable when a game is removed; new activation or
launch with that missing registration is rejected until its context is
reassigned. The catalogue supports 256 entries, with a separate 512-rule storage
bound. Windows observation takes one process inventory for a catalogue cycle;
custom games are excluded from simulator telemetry polling.

Recovery/fault and explicit deactivation block automation. Manual mode leaves activation under operator control until the operator explicitly selects Automatic. Without an active owner Cockpit shows **No profile active**. Manual game launch uses the active profile, or Default if no profile is active; retained selection metadata cannot choose a different launch profile. Automatic selects the unique highest-priority matching enabled rule; no match selects Default. A tie reports all highest-priority game IDs and holds the current safe selection, including when those rules name the same profile. Missing or ineligible targets are actionable holds.

The reconciler records attempts, including failed native admissions. An unchanged context does not repeatedly acquire hardware or replay commands. New native process sessions or explicit operator/rule changes allow a new attempt. Saving an inactive draft leaves the active owner alone; saving the active profile reapplies its saved changes while preserving mode and game context. Every activation still requires the ordinary coordinator, fresh native baselines and separately qualified backend capabilities.

Verification: all 100 portable profile tests pass. The 15-module profile gate passes at its unchanged 71% minimum; selection policy is 147/147 production lines (100%). Evidence: `.research/profile-game-selection-coverage`. This establishes policy and idempotence; native process watching, coordinator composition, launch preflight and Windows smoke acceptance are separate work.

Independent native process capture is integrated as `c81b72f`. Five native tests include an actual GamesRuntime observing external startup/exit with no UI or integration setup and no provider/device calls. Native19 gates pass (observer 99.28%, runtime 94.74%). Reads use cached observations; two exact observations confirm a new process, and exit, replacement, ambiguity or stale capture revokes it.

The machine-local rule store has its own strict bounded schema, separate from portable graph storage. Missing state remains Manual without creating a file or activating Default. Atomic replacement, current rules and legacy selection metadata round-trip; malformed/unknown/oversized or nonregular state is preserved as an error. Legacy manual-profile fields are compatibility metadata, not a separate user selection or Manual launch authority. Process IDs, epochs and sessions cannot be persisted as rule authority. Two store tests and the focused native gate pass: 109/115 production lines (94.78%), `.research/profile-automatic-store-coverage`. Coordinator admission and launch composition remain subsequent work.
