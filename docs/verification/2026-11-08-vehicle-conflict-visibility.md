# Vehicle conflict preview visibility

The integration-enabled profile editor now pauses its debounced conflict
compile while the document is hidden. Hiding clears pending diagnostics and
invalidates any compile or diagnostic page still in flight. The visible
editor checks the latest draft and inventory revision after reveal.

The native baseline sent one extra compile after a private DCS catalogue
change while hidden. With the fix, the same test sent zero hidden compiles.
Both running clients compiled the current draft after reveal, preserved
its unsaved rename and layout, and reopened on all six authored aircraft
inputs. Neither client's fixture started a simulator or applied a profile.

The profile vehicle suite passed 73 tests. The integration coverage suite
passed 68 tests and its gates. The conflict controller reached 98.18% line,
88.46% branch and 87.50% function coverage. Svelte reported zero errors and
warnings. The production frontend build passed.

The native executable SHA-256 was
`c6382209154e118311c6ec0818794d74e9159bf6e8231663488e9c2b3f812ae6`.
The browser-dev gateway SHA-256 was
`2cd89706b01e0b37f29007996d118827dc5e613538c18b804a3ff1207747ac7b`.
