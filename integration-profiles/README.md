# Profile examples

Portable routing now uses the unified Profile schema2 document. Open **Profiles**, create or copy a profile, author its explicit graph and save before Apply. Game-owned provider settings belong in each game's details.

See [`complete-rig.json`](../tests/fixtures/profiles/complete-rig.json) for the assembled portable fixture and [`complete-rig-replay.json`](../tests/fixtures/profiles/complete-rig-replay.json) for its pure replay evidence. Machine-local resource assignments and exact observed port hashes remain separate from portable JSON. These are software fixtures; actual devices, installed game catalogues and qualified backends must be assigned explicitly.

Independent Integrations/DCS/mapping profiles and their start controls are retired. Old configuration is not read or migrated. Retained ownership journals have explicit restore-only cleanup and are never a reason to resume an old graph.
