# Discovery presentation and unchanged imports

Verified against the running native Windows cockpit at `http://127.0.0.1:1420`.

- Discovery titles match game tile titles: 15px, weight 600, line height 1.45, with the title aligned to the image's top edge.
- Empty result groups are hidden. Empty libraries remain visible while scanning or when they report a diagnostic. Manual match and installation controls have been removed from review rows.
- Runtime candidates compare as normalized sets, preserving the selected executable. Reordered candidates, path casing and trailing separators do not create updates. Different source observations for one root produce one installation.
- Battle.net artwork matches the exact product and excludes localized, expansion, logo and tiny assets. The installed Diablo II: Resurrected base-game asset produces a 466×960 preview, displayed in a 215×119 image frame in the running cockpit.
- Steam local artwork selects the largest usable image, including nested cache files. Discovery previews retain up to 960 pixels and 128 KiB, with no upscaling. Manual background limits remain unchanged.
- Automatic platform previews persist outside registry metadata. Existing imported games receive refreshed artwork without an artificial Update action; a manual background retains precedence.
- Preview memory, review response bytes and cache file counts remain bounded. Unchanged installation artwork shares the preparation budget.

Validation completed:

- All 1,173 frontend tests passed; cockpit type checking and production build passed.
- The full native regression suite passed, followed by focused discovery and artwork reruns for the final refinements.
- Native discovery acceptance covered copies, moves, removals, accordion state and unchanged multi-executable imports. The real MCP SDK scan, pagination, Apply and replay acceptance passed.
- The live presentation check confirmed identical title sizes, exact top alignment, nonempty result sections, corrected D2R artwork and no Update for its unchanged installation.
- Games frontend coverage passed its 71% gates. Native discovery and artwork production modules passed their 71% gates, including 100% reconciliation, over 94% worker coverage and over 98% installation artwork cache coverage.

Local evidence is in `.research/game-presentation-*`, `.research/game-discovery-native/evidence.json` and the coverage summary directories.
