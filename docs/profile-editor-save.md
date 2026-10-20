# Atomic profile editor save

`profile_editor_save` accepts `{document, expectedRevision, assignments}` and returns `{document, assignments, profiles, session, saveApplication}` (including bounded real library summaries and their native semantic hashes). The editor owns its Add/Copy/Cancel draft until Save succeeds. Saving an inactive profile only stores it and never activates it. Saving the active profile prepares and reapplies the saved graph using its existing game context and selection mode.

`saveApplication.state` distinguishes `inactive`, `applied` and `failed`. A failed reapply does not undo successful storage; its error remains actionable and the user can retry the saved changes. Newer native lifecycle operations take precedence over an in-flight save. Success is reported through the shared notification area rather than a persistent saved-feedback banner.

An explicit `expectedRevision: null` creates only a new canonical UUID at document revision 1. Existing IDs and capacity overflow are rejected. A numeric expected revision must match both the saved graph and the submitted document; a successful replacement increments it once. Missing, unsafe, stale and mismatched revisions are rejected.

The complete portable graph and its separate machine-local assignments validate together and replace one bounded library envelope in one atomic write. Every assigned logical resource has a unique exact expectation for each of its ports. A zero-port seeded resource may have an assignment with no port expectations; an unassigned resource remains an editable graph node. Invalid graph/pins, read-only storage and failed replacement leave memory, saved revisions and assignments unchanged.

The fixed Default UUID retains the name `Default`, while its graph remains editable. Its name and removal are protected across native mutation paths and library loading. A custom UUID named `Default` is an ordinary named profile. Native import, export and duplication exclude local assignments; the editor Copy likewise starts without them.

A document remains limited to 512 KiB. Only the atomic editor request can carry up to 2 MiB to include exact local port expectations; unrelated commands keep 64 KiB and other profile requests keep 576 KiB. The browser bridge and native service enforce bounded framing plus the command-specific limits. A shared valid 1,024-port fixture exercises a combined save above 576 KiB through the pure Service, native Manager and actual service process.

The library has both a 32-profile count limit and a 17 MiB total serialized byte limit, including local assignments. These are independent limits: 32 simultaneously maximal graphs with maximal local metadata are not promised. A full library reports an actionable error and preserves the existing saved state.
