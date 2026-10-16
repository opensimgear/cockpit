# Companion C0 foundation — 16 October 2026

The retained foundation consists of portable contracts, bounded session/ownership rules, the shared renderer and the mobile Tauri application boundary. See [the current architecture](../companion-uis.md).

Historical native desktop-shell build and qualification instructions were retired when desktop companions moved to the [browser](../browser-companion.md). They do not apply to current builds. Android API 29 and iOS/iPadOS 15 remain candidate build floors, with device qualification pending #55/#56.

C0 contract tests covered publication hashing, input ordering, ownership, neutral baselines, cancellation, lease expiry and peer isolation. Local recording and renderer checks do not establish physical multitouch, LAN or mobile lifecycle qualification. No host output qualification follows from these checks.
