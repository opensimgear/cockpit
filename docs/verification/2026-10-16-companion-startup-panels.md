# Companion startup panel restoration — 2026-10-16

The persisted native assignments could outlive the in-memory publication map. USB detection and the saved listener started before the Companions page uploaded IndexedDB panels, producing a missing-publication error that remained in USB status after recovery.

Changes:
- Native uploads persist a bounded validated panel cache atomically. Cache restoration revalidates hashes/assets before transports start; invalid cache content is retained and rejected.
- Saved network listeners and automatic USB reconnection wait until every persisted assignment has matching content.
- App startup restores missing legacy assignments from the existing local library, independent of the active page. A shared promise prevents competing startup uploads.
- The designer no longer reuploads the entire archive when opened.
- USB host errors are removed when the device reports a recovered live panel or awaits assignment; pending/error states retain the diagnostic.

Checks:
- Windows companion service: 71 passed.
- Windows companion host: 32 passed.
- Native cache regressions: 3 passed, including real saved-assignment restoration, existing-file replacement, missing-content admission gate and corrupt-content rejection.
- USB serial suite: 12 passed, including recovery-only error cleanup.
- Frontend startup regressions: 3 passed, covering native-cache fast path, legacy selective/deduplicated restoration and actionable missing-content failure.
- Cockpit/browser Svelte checks: zero errors/warnings.
- Linux service run: 60 passed, 1 ignored, 1 failed because Secret Service is unavailable; the corresponding Windows suite passes.

Windows mirror updated and native app rebuilt. First launch on Overview migrated the assigned legacy panel into companion-panels.json (one validated entry), without opening Companions. A second restart kept the cache unchanged and showed no startup notifications, confirming the native restore fast path rather than a frontend reupload. The Sunton was listed offline during this run; physical connection confirmation was requested, so live board reconnection is not claimed by this verification.
