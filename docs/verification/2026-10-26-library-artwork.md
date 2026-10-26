# Imported library artwork

The image editor lists cached artwork associated with an imported game's exact
library identities. Steam headers, covers, heroes and logos, Battle.net product
artwork in each cached locale, and named installation images can be selected
before using the existing 16:9 alignment frame. Current and default images stay
available alongside disk and URL imports.

Browsing is passive, bounded to 64 deduplicated choices, and returns opaque IDs
and small JPEG thumbnails. Selecting a choice loads its bounded 960-pixel JPEG.
Saved images retain that preview's bytes and crop. Metadata requests allow the
existing 128 KiB artwork budget after base64 encoding; oversized requests and
invalid images remain rejected. Thumbnail caching avoids decoding every image
again when reopening the editor. Saving is disabled during selection.

MCP exposes the same flow through `game_artwork_list`, `game_artwork_preview`
and `game_background_save`. Saves return bounded, replayable receipts.

Validation on 2026-10-26:

- Native library suite: 1,331 passed, 10 environment-specific checks ignored.
- Native game-library coverage: every module passes 71%; artwork browsing is
  100%, library artwork enumeration 94.57%, and managed artwork 87.73%.
- Frontend game coverage: 98.55% lines. Frontend suite: 1,175 of 1,176 passed;
  the unchanged dense profile-diagram routing timing test exceeded its 1.5-second
  limit (about 2.1 seconds). Its routing correctness assertions passed.
- Svelte type checking and production build passed.
- `tests/ui/game-artwork-native.mjs` passed against the actual native Cockpit
  and MCP server: all 12 cached Diablo II choices, an image exceeding 64 KiB,
  selection loading guard, crop movement/resizing, save/reopen, cancel, MCP
  pagination and operation replay. No game was launched; the previous background
  and MCP settings were restored. Evidence is in `.research/game-artwork-native/`.
