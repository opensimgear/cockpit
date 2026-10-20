# Unified profile contracts and storage

The portable document, native library, session metadata and typed frontend client
support [the unified profile plan](https://github.com/opensimgear/cockpit/issues/79).
See the [Profiles guide](profiles-guide.md) for creating, editing, applying and
inspecting a rig, and [editor details](unified-profile-editor.md) for exact routing.

The Profiles section opens to a list containing Default and an **Add
profile** button. Opening a profile opens its editor; **Back to Profiles**
returns to the list. The client keeps that editor context independent of
runtime selection. Opening, creating, editing, saving or closing an editor
does not activate it.

## Documents and ownership

`crates/profiles` is the native authority. `src/lib/profiles.ts` declares the
matching frontend contract and client. Shared JSON fixtures under
`tests/fixtures/profiles` are checked by both implementations.

Schema version 2 contains:

| Field | Meaning |
| --- | --- |
| `id`, `name`, `revision` | Canonical UUID identity, display name and optimistic saved revision |
| `eligibility` | Logical game catalogue requirements, without executable paths or live processes |
| `resources` | Required/optional logical devices, companions and game resources, with typed ports |
| `bindings` | Explicit directional source-port to destination-port connections |
| `processingNodes` | Visible typed processing declarations compiled for the applied session |
| `virtualOutputs` | Logical virtual controller definitions, without machine-specific vJoy slot IDs |
| `desktopOutputs` | Optional keyboard and mouse endpoint with exact typed action ports; no OS process or local enablement |
| `visibility` | Intent referring to logical resources, without HidHide instance paths |
| `overrides` | Optional peripheral runtime intent, separate from permanent calibration/firmware setup |
| `companionAssignments` | Logical companion assignments and immutable publication hashes, without pairing secrets |
| `editor` | Node positions and viewport, independent of execution semantics |

Inputs consume values and outputs produce values. Ports distinguish **state**,
**event** and **action** delivery. They declare a value type, applicable unit,
numeric bounds or enum values, and state freshness/fallback behavior. Unknown
state is distinct from an ordinary value. Events/actions do not become retained
state merely because they pass through a profile. A stale state uses its declared
fallback; this contract does not authorize replay of an old event or command.

Native validation rejects unknown versions/fields, malformed identifiers, invalid
port declarations, duplicate graph identities, missing endpoints, incompatible
connections, implicit type/unit conversion and cycles. Runtime compilation and
capability/resource admission remain separate from document validation.

Live route observations, hardware identities/evidence, OS device paths, backend
slots, pairing credentials, process IDs, raw calibration, game installation and
provider connection settings are not portable graph fields. Local resource
assignment records are stored separately from document content and are not
exported. Imported or duplicated documents never acquire the source's local
machine assignments automatically.

## Default, revisions and session state

The fixed built-in Default identity is
`00000000-0000-4000-8000-000000000001`. It is editable and cannot be deleted.
Fresh startup selects Default without starting a hardware session. Named profiles
can start blank or duplicate another document. Copies get new identities and
remain independent of subsequent source edits.

Saved documents and captured applied revisions are separate values. Save, rename
and delete use `expectedRevision`; a stale revision is rejected before storage
changes. Saving a draft cannot mutate an already captured snapshot. Editor-only
changes do not change the semantic hash, which excludes layout and document
identity/name/revision metadata.

Session metadata separates selected profile, manual/automatic selection mode,
stopped/faulted state and the captured applied revision. Automatic selection must
honor manual mode, Stop and faults. Selecting a document cannot rearm processing.
Pure snapshot/session metadata does not submit output. The native coordinator
owns preflight, acquisition, execution, neutralization, compensation and recovery.
Process restart never replays a saved applied snapshot. Automatic selection, when
explicitly enabled, independently requires current verified native observations.

## Fresh bounded storage

The native service uses a fresh `profiles-v2` directory beside the other application
configuration, containing its own library. It never reads or migrates old game,
mapping, integration or companion-feedback profile configuration into that
library. Independent game-profile, mapping, DCS/integration and companion-feedback
authoring and activation paths are retired. Existing ownership journals are
cleanup-only evidence, never compatibility configuration readers for this schema.

Each portable document is limited to **512 KiB**, the library to **32 profiles**
including Default, and its complete serialized envelope to **17 MiB**. A document
revision must be a positive integer representable exactly by JavaScript.
Collection, string, port and graph limits are validated independently.

Writes validate a prospective complete library before atomically replacing the
stored file. A failed validation/write leaves the prior committed library and
in-memory state unchanged. Interrupted temporary files are not loaded as the
committed library. Unsafe symlink/reparse storage paths are rejected.

A corrupt or unsupported stored library is retained. The service exposes
diagnostics and a read-only Default recovery view; it does not silently overwrite
the corrupt source. `profile_reset` requires `confirm: true` and replaces only
the new profile library. It does not alter drivers, device firmware or unrelated
configuration.

## IPC

All operations use the existing native gateway manager in desktop and browser
development. Profile requests have a dedicated frontend queue and return
`state.profiles` without querying gateway inventory or acquiring controllers.

| Command | Fields |
| --- | --- |
| `profile_list`, `profile_status` | None |
| `profile_get`, `profile_export` | `id` |
| `profile_create` | `name` |
| `profile_rename` | `id`, `name`, `expectedRevision` |
| `profile_duplicate` | `id`, optional `name` |
| `profile_delete` | `id`, `expectedRevision` |
| `profile_save` | `document`, `expectedRevision` |
| `profile_import` | `document`, optional `name`; creates a fresh identity |
| `profile_select` | `id`, `selectionSource` (`manual` or `automatic`); metadata only |
| `profile_selection_mode` | `mode` (`manual` or `automatic`) |
| `profile_apply` | Saved `id`, `expectedRevision`, optional logical `gameId` |
| `profile_game_launch` | Logical `gameId`; selection and process authority are native |
| `profile_stop`, `profile_recover` | None; cleanup does not replay the graph |
| `profile_reset` | Explicit `confirm: true` |

Lists/status return compact summaries and session metadata; get/export return one
document. Unknown operation fields are rejected. The validated IPC client identity
is removed before the strict profile operation envelope is parsed.

Normalized profile requests have a 576 KiB limit, while unrelated gateway requests
retain their existing 64 KiB bound. The browser/native correlation envelope has
separate bounded framing; browser profile responses are capped at 2 MiB, with the
existing 128 KiB response cap retained for unrelated commands.

## Validation

Run `pnpm profiles:coverage` for frontend contract/client coverage and
`python scripts/profiles-coverage.py` for the per-module native coverage gate.
CI runs native profile checks on Windows, Linux and macOS, as well as frontend,
gateway and browser transport regressions. Native profile checks require at least
71% production line coverage in each measured module.

Fixtures cover Default, a blank document and a mixed-routing graph. Tests include
schema/round trips, document and library limits, corrupt storage, interrupted
writes, optimistic revisions, copy independence, import/export boundaries,
immutable snapshots, stopped/faulted selection and editor navigation. These are
contract/storage checks; they do not qualify driver, hardware or game execution.
