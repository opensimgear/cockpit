# Retained ownership cleanup

Unified Profiles replace independent mapping authoring, mapping starts, and standalone companion feedback routing. Their obsolete configuration is not loaded by `profile_retired_recovery::Recovery`. This native adapter keeps only the ownership records needed to finish interrupted cleanup.

Construction takes the native mappings directory and creates no feeder, capture, mapping runtime, feedback dispatcher, driver, or background worker. Status reads only `output-recovery.json`, bounded `companion-recovery/instance-<32 lowercase hex>.json` files, and protected native provisioning journals. Missing storage stays missing. Every existing path ancestor rejects symlinks and Windows reparse points; journal and lock paths must be regular files. Malformed, unknown, redirected, or excessive retained records fail closed without modification.

A native output reference pins its logical journal identity, complete `RunToken` (canonical UUID and decimal-string generation), and SHA-256 of the whole record. Cleanup compares that complete record after taking the journal store lock and before constructing a feeder. A confirmed record uses the existing cleanup-only `Recover` message, terminates the helper, and writes Clean only after acknowledgement. No report, Start, recording replay, or normal output run is reconstructed. A missing reply leaves Releasing ownership pending; it is never automatically retried.

Intent and Releasing gaps permit only a separate, explicit exact-reference no-output acknowledgement. Qualification gates still apply to recovery and acknowledgement. Provisioning exposes only the existing qualified Recover operation for its exact native operation reference and cancellation of the exact active cleanup job. It cannot create a slot, layout, or provisioning operation. Shutdown requests cancellation only for an existing cleanup job.

The five strict IPC operations are `profile_retired_recovery_status`, `profile_retired_recovery_recover`, `profile_retired_recovery_acknowledge`, `profile_retired_recovery_provision_recover`, and `profile_retired_recovery_provision_cancel`. Mutation calls forward a native reference unchanged; paths and obsolete configuration are never accepted. Status is bounded and reports pending records and diagnostics without acknowledging them. Manager admission must reject new Apply, launch, and input learning while retained ownership or an ambiguous record remains; cleanup mutation also requires idle unified ownership.

Legacy game cleanup uses a native read-only `capture_mapping_owner(owner)` followed by `recover_mapping_owner(owner, expected_reference)`. A null capture must still be clean when cleanup is requested. A present capture must remain the same manual journal, owner, full token, and record hash. The old game's baseline token and recording are never used. Exact visibility restoration and companion assignment restoration remain with their existing native owners; neither can restart a retired mapping, integration, or feedback configuration.

The focused tests cover unchanged obsolete configuration, bounded inventories, malformed records, real Windows junctions, nonregular journal/lock files, exact owners and record races, competing journal locks, explicit acknowledgement, qualification rejection, cleanup reply loss, provisioning/cancellation, strict request parsing, and two-step retained game cleanup. Tests establish software behavior; physical backend qualification remains unchanged.

The source-stamped native gate in `.research/profile-retired-retention-coverage`
passes all 16 recovery tests at 352/391 production lines (90.03%), above the
unchanged 71% module threshold. The exact locked Session helper separately passes
all 85 virtual-controller tests and all 23 module gates; Session coverage is
245/265 lines (92.45%). Manager/UI retirement composes this primitive separately.
