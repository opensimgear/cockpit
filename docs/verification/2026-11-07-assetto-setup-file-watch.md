# Assetto Rally setup file notifications

The running Windows Cockpit configured telemetry for a private Rally
installation with only a launcher. The fixture then observed the exact
`games.detail.assetto-corsa-rally` read model while creating and removing the
nested runtime. A sibling with different bytes invalidated the configured
fingerprint. Removing it restored the original launcher state. A sibling
containing identical bytes also invalidated the fingerprint because the
resolved runtime path changed. Removing it restored the launcher again.

| File change | Native WebView | Browser-dev |
| --- | ---: | ---: |
| Create nested runtime and its directories | 51.9 ms | 58.5 ms |
| Remove the nested runtime | 62.9 ms | 55.9 ms |
| Create a sibling with identical bytes | 61.9 ms | 56.0 ms |
| Restore the launcher as the selected runtime | 60.2 ms | 53.3 ms |

These are single-run filesystem-operation-to-read-model observations. Both
transports made zero `game_get_state` or `game_integration_check` calls
during the observation. The four sequential transitions also fit inside the
fixture's four-second bound, which is shorter than the two-second fallback
could service four separate changes. The fixture removed its provider setup
and catalogue entry. Its files were authored for the test and never launched.

The frontend was based on `a145a9b2`. The native dev executable SHA-256 was
`0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`.
The directly linked browser-dev gateway SHA-256 was
`2272c6b0035edb1470da3155b733c9b3a6bbccd14d996660a41b353fdba66242`.
Eight focused native watcher and setup tests and all 12 game read-model tests
passed. No production code changed.

The first native attempt needed to wait for the application shell. The next
attempt distinguished the initial setup job's configured diagnostic from the
passive fingerprint diagnostic used after the first file change.
