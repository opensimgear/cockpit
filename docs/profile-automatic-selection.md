# Native automatic profile selection

The portable policy accepts only native verified game observations and saved rules. A rule contains one supported game, its validated installation executable identity, a saved unified profile, an enabled flag and an explicit signed priority. PIDs and creation epochs are transient native observations, never saved rule authority.

Recovery/fault and Stop block automation. Manual selection stays pinned until the operator explicitly selects Automatic. Automatic selects the unique highest-priority matching enabled rule; no match selects Default. A tie reports all highest-priority game IDs and holds the current safe selection, including when those rules name the same profile. Missing or ineligible targets are actionable holds.

The reconciler records attempts, including failed native admissions. An unchanged context does not repeatedly acquire hardware or replay commands. New native process sessions or explicit operator/rule changes allow a new attempt. Saving a draft does not reapply the current graph. Every selected intent still requires the ordinary coordinator, fresh native baselines and separately qualified backend capabilities.

Verification: all 100 portable profile tests pass. The 15-module profile gate passes at its unchanged 71% minimum; selection policy is 147/147 production lines (100%). Evidence: `.research/profile-game-selection-coverage`. This establishes policy and idempotence; native process watching, coordinator composition, launch preflight and Windows smoke acceptance are separate work.

Independent native process capture is integrated as `c81b72f`. Five native tests include an actual GamesRuntime observing external startup/exit with no UI or integration setup and no provider/device calls. Native19 gates pass (observer 99.28%, runtime 94.74%). Reads use cached observations; two exact observations confirm a new process, and exit, replacement, ambiguity or stale capture revokes it.

The machine-local rule store has its own strict bounded schema, separate from portable graph storage. Missing state remains Manual/Default without creating a file. Atomic replacement, current rules and a manual pin round-trip; malformed/unknown/oversized or nonregular state is preserved as an error. Process IDs, epochs and sessions cannot be persisted as rule authority. Two store tests and the focused native gate pass: 109/115 production lines (94.78%), `.research/profile-automatic-store-coverage`. Coordinator admission and launch composition remain subsequent work.
