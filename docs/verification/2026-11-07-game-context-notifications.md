# Retained game context notifications

The game context worker now observes one retained counter before it checks
its sources. Library commits, process notifications, setup completion and
filesystem callbacks all advance that counter. A change during inspection
causes another pass even if an unrelated wait consumes the thread's wake
token. The library and process revisions remain separate so a file write
keeps an otherwise current process inventory reusable.

The native SDK file fixture checks that an unrelated sibling write stays
quiet and that the selected file advances the worker counter. It consumes
the wake token and verifies that the old counter still prevents parking. A
file change leaves both the library and process revisions intact. The DCS
filter fixture checks Saved Games profile changes and catalogue JSON names.

## Running Windows Cockpit

The desktop development executable had SHA-256
`ab7eb1ffa0ceecc8dbe757f439ca7f0059b2980cbf092827ed5ea7b3e2df7c95`.
Each run used an isolated Cockpit database and a private game installation.

| Fixture | Result |
| --- | --- |
| `tests/ui/dcs-catalogue-background-native.mjs` | A provider-path change, an edited module and an alias remap updated the open aircraft editor. The editor recovered after a failed directory registration and caught another file change on reopen. Saved data, draft text and layout stayed intact. |
| `tests/ui/game-process-boundary-native.mjs` | 20 expected transitions reached both the shared stream and the Active game header. The wrong executable and the retired path were ignored. The trace recorded zero repeat UI queries. |

The process fixture used the existing Windows process-discovery fallback.
Shared stream delivery took 796–1,061 ms for starts, 291–342 ms for exits,
and 62.03 ms for withdrawal after executable replacement.
Native process-event consent and unavailable-watch behavior remain separate
acceptance items.

## Local suites and coverage

Windows passed 2,421 native tests with 18 default ignores. Both pinned
DCS-BIOS archive tests passed separately. All 1,705 frontend and timer-guard
tests passed. Coverage for the five changed native modules was 91.51–100%
for production lines and 70.31–100% for LLVM functions. The Rust build did
not instrument branches. The local logs and profiles are under
`.research/reactive/retained-game-watch/`.
