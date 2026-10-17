# Profile-owned companion panel authoring

The selected companion block now has an explicit retained-panel picker. Capturing a publication adds only its immutable hash to the draft. It does not install a panel, create controls/routes, save the profile or activate output.

Source controls and declared feedback inputs come from the same native publication translator used by runtime preparation. Panels retain the real 1–128 interactive-control requirement. A graph may select only feedback inputs from that real panel; no dummy control is inserted. Visible/enabled/navigation actions are not invented.

Replacing a captured revision with selected graph ports requires explicit removal of those companion ports and their routes, or explicit reconciliation of every existing logical port. Reconciliation can assign a copied graph to a chosen paired companion while that companion still displays another panel. Each selected descriptor/hash must appear on a captured native page; all pages must share the exact resource, desired publication, capability fingerprint, paired identity fingerprint and catalogue size. Compatible graph routes remain, and machine identities stay in local assignments.

The dedicated picker replaces the generic current-publication port picker for companion blocks. Port catalogues remain paged and only selected ports enter the graph. Revision/instance assignment and exact port edits participate in existing undo/redo. Browsing, opening, navigation and Cancel issue no Save or Apply.

## Native boundary

`profile_companion_catalogue` reads bounded metadata through `metadata(false)` and the new passive Hub summary getter. It does not call effectful operator status/session expiration, construct a listener or copy publication assets. The catalogue fingerprint covers the complete retained summary list.

`profile_companion_inspect` accepts a native resource UUID, publication hash and bounded page. Manager resolves exactly one retained CompanionInstance identity from cached registry evidence; browser instance IDs/tokens and unknown fields are rejected. The helper checks actual pairing and desired publication, derives exact controls/feedback, revalidates the native proof, and returns prospective descriptors with `current:false`, empty route IDs and no tokens. Existing companion assignments are unchanged. Actual Apply still requires fresh native preparation, ownership and baselines.

## Evidence

- Five native helper tests, one strict Manager-parser test and one cached resource-identity test pass. The native authoring gate covers 48/51 production lines (94.12%), with source stamps for Hub metadata, paired preparation and native port derivation.
- Eight SDK/paging/graph tests pass. Dedicated coverage: 91.92% lines, 97.33% branches and 95.83% functions; the existing 71% thresholds remain.
- Seven tests execute actual Svelte handlers and SSR output for passive opening, explicit capture/selection, feedback-only routing, replacement, copied-profile reconciliation, stale/disposed reads, and undo/redo.
- All 51 combined profile UI tests and 31 gateway tests pass; transport checks verify read-only authoring does not wait behind Save or delay emergency Stop.
- Desktop and browser Svelte checks and production UI builds pass.

These are deterministic native fixtures and SSR/handler checks. They do not establish browser-companion renderer end-to-end acceptance, device/driver qualification, simulator submission or game confirmation. Those P5 acceptance items remain separate.
