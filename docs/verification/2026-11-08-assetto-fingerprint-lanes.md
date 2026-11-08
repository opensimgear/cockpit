# Assetto fingerprints and provider setup readiness

Each Assetto variant now has its own fingerprint cache slot. Setup,
background inspection, and active validation use the same slot for that
variant. An invalidation, content hash, or metadata read for one variant
does not delay a different variant. Each slot retains one executable
identity. Explicit checks still force a hash, and a changed executable
cannot publish a stale identity.

A provisioning job withholds source readiness and passive reader admission
for the provider recorded in that job. The job's provider can span multiple
installed titles. A different provider keeps its own committed setup
evidence. Setup edits and provisioning still share exclusive admission.

The fingerprint regression holds the Corsa slot and verifies Rally can
complete a forced executable check and publish its background setup
baseline. The readiness regression starts a DCS job and checks that both
DCS installation scopes withhold source proof while Rally remains ready.

The full Windows rerun passed 2,469 tests with 18 default skips. The 198
focused native tests and 51 related frontend tests passed. A native
file-watch fixture and an MCP loopback fixture failed on the first full
run. Both passed in isolation and in the full rerun. The coverage merge
excluded one invalid subprocess profile.

Changed production modules reached 76.62% to 97.37% line coverage. The
Assetto setup and setup cache modules reached 84.38% and 95.45% of named
production functions respectively. LLVM's raw function records, including
closures, measured 70.97% and 62.34%. LLVM emitted no branch counts.

The rebuilt Windows dev Cockpit passed the authored DCS fixture with no
cleanup errors. Assetto Rally recovered a failed watch and published all
four executable fixture changes without state queries. Both X-Plane SDK
fixtures stayed ready and published catalogue revisions after external
writes. Executable SHA-256
`0b550fa4c7acd1c196368c95d0d8335083f875a45f02feed6de33e7323285954`.

Physical controller and simulator acceptance remain in #199. The
VKB sweep, preview cadence, unplug/replug, direct DCS effects and
bidirectional browser panel checks are tracked there with references
from #191, #193, #195 and #196.
