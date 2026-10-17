# Saved profile companion publications

`profile_companion_references` is a passive, field-free metadata request. Its
`profiles.publicationReferences` contains schema version 1, a SHA-256 fingerprint
of the current native library and sorted unique publication hashes from every
saved profile. Editor selection, an unsaved draft and current Hub assignment do
not stand in for these references. Corrupt/read-only profile storage rejects the
read so compaction must preserve existing publications.

Native `companion_prune` unions the caller's retention list with this saved set.
It holds the profile library lock from capture through the Hub prune, serializing
concurrent Save/Delete. Browser omissions cannot remove a saved profile's panel.
The Hub separately retains current assignments, sessions and profile ownership
baselines. If the combined set exceeds the existing 100-hash prune bound, the
operation fails without pruning rather than truncating protected references.

The source-stamped native gate in `.research/profile-retired-retention-coverage`
passes four retention tests at 26/35 production lines (74.29%), above the unchanged
71% gate. Tests run actual saved profile documents through the real Hub prune,
verify concurrent library mutation waits, reject malformed/read-only references
without reaching prune, and preserve an oversized genuine saved reference set.

All 102 portable profile tests and all 15 production-module gates pass in
`.research/profile-companion-references-coverage`; Service is 353/360 lines
(98.06%). Its two new tests cover complete-library/deduplicated references,
restart and deletion, passive reads, no runtime activation, strict request fields
and unchanged corrupt storage. This metadata does not acquire companion delivery.
