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

Fresh development databases include these five widget sources, the editable
panel layout and its compiled preview alongside the Toggle widget. They are
seeded atomically with the example profiles. Startup preserves existing edits;
reset the development database to pick up changed examples. Pairing and panel
assignment remain explicit actions in the Companions page.

`workspace.json` is generated from `panel.mjs` by
`node scripts/dev-companion-seed.mjs`. Run the same command with `--check` to
verify it matches the current sources and compiler; this also runs in the
shared fixture checks. New destinations should use the panel editor's live
size warning to choose their own page dimensions before compilation.
