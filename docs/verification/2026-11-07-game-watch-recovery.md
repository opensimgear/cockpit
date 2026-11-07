# Native game file-watch recovery

The Windows dev Cockpit reproduced a lost DCS catalogue watch after a
temporarily blocked directory was restored. The open editor eventually
showed the restored catalogue. A subsequent module edit while the editor
was closed left the catalogue generation unchanged on the prior build.

The context owner now schedules the earliest failed registration deadline
across its DCS, Assetto and X-Plane file watches. Successful registration
clears the deadline and refreshes the source baseline. Failed retries on
the same scope leave the generation alone. Removing a scope cancels its
retry. Native tests exercise repeated registration failure, repair and
scope removal.

The rebuilt Windows WebView and Edge browser-dev passed the DCS fixture.
Both recovered the selected path while the editor was open and displayed
the next background edit on reopen. The fixture preserved the provider
revision across recovery and kept the saved profile and assignments
intact. It used private catalogue files and a private database.

The baseline desktop executable SHA-256 was
`8bc0f11da8f59b0648759cba6337dbdd7396f71113889ac60b036cf941c7978e`.
The rebuilt desktop and browser-dev gateway hashes were
`b8e875f2dd91e38c8acd2d8c8c3f4244e731de731fae4e86f19bf757a4e87a9c`
and `4e6e6f7fb4156b18f3327f1f34b3e293ebf69281e7b288ac23b42524b9e11893`.
Windows denied replacement of Cargo's top-level gateway executable alias.
The browser-dev fixture ran the newly linked executable from Cargo's
dependency directory. Raw logs are under
`.research/reactive/context-watch-recovery/`.

All 2,409 Windows tests passed with 18 default ignores. Both pinned
DCS-BIOS archive tests and 12 frontend resource tests passed separately.
The repository production line counter measured 91.56% for game context,
96.45% for the shared watcher, 100% for Assetto and 82.14% for DCS.
LLVM's supplementary function report measured 71.01% across those files,
with 65.91% for game context and 62.50% for DCS. This build recorded no
branch counters.

Windows process-monitor consent, actual simulator delivery and the
remaining acquisition exceptions stay in their existing acceptance issues.
