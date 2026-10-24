# WIN Companion demo panel

`panel.mjs` exports editable widget `sources` and a panel `draft` for the existing
`compilePanelDraft` API. The page is 2400 × 1440 pixels, sized above WIN Companion's
observed 2184 × 1311 physical-pixel display area. Placements retain their normalized
bounds, so the layout scales together. Compile with motion and audio disabled.

The four momentary controls release on both pointer release and cancellation.
Altitude, rudder and panel-brightness bars hide unavailable feedback; the altitude
warning also distinguishes unavailable data. Static gauge frames are separate
from their sampled bars so the larger artwork fits the existing compiler's
manifest and memory limits. The compiler and its limits are unchanged.

The saved Cockpit workspace contains these sources, layout and compiled
publication. The paired WIN Companion has that publication assigned. New
destinations should use the panel editor's live size warning to choose their
own page dimensions before compilation.
