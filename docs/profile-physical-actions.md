# Native physical action leases

The profile output adapter uses the existing guarded Direct USB and CAN workers. Preparation reads their cached, exact route and catalogue, requires an actual native Invoke preparation pin, and selects a readable Setter on the same advertised physical endpoint. The endpoint must advertise both `restorePolicy: "if-unchanged"` and `fallbackPolicy: "capture-at-claim"`, with a lease duration supporting the adapter's bounded 5-second TTL. Older endpoints stay visible; they cannot acquire profile-owned output leases. No qualification flag is changed by this adapter.

Before any claim, capture produces a bounded (8 KiB per endpoint) receipt containing OS-protected cleanup authority. The profile coordinator's existing durable manifest is the only journal. A receipt contains neither a graph nor an invocation argument tuple. The first `claim_current` atomically captures the device's actual prior state. The adapter never supplies a stale host-read fallback and never falls back to the legacy claim command. Exact native connection, module boot, configuration, action catalogue, endpoint and contract pins are checked around every worker result.

Delivery needs a separately bound, current native Invoke token and the exact profile run owner. Ordered named arguments are checked against both the canonical scalar contract and the device's original descriptor. Integer scalars are encoded losslessly as native integer JSON values. One-shot aliases may use the lease of an advertised readable Setter on the same physical endpoint. An uncertain claim, invocation or renewal revokes delivery; the operation is not repeated. The bounded owner worker is responsible for dispatch queues, monotonic renew scheduling and immediate cancellation before this primitive is connected to Host.

Cleanup reads current state and bounded endpoint ownership pages, then sends only the token-fenced release for the exact still-owned lease. Firmware compares its actual output with the last value successfully read back under that lease before applying the captured prior value; external edits are preserved. An uncertain release is not reissued by the same adapter. Explicit recovery may only inspect and release an existing lease; recovered objects cannot claim, bind delivery or invoke. A later owner's lease is preserved. Changed or unavailable route, boot, configuration or catalogue remains an actionable unresolved receipt instead of selecting another device or replaying output.

Publication changes can advance a CAN delivery lease during cleanup. A sealed
cleanup-only proof permits passive state/ownership reads and exact-secret Release
through the current route of the same gateway/module, boot, configuration,
profile and action catalogue. It never authorizes Claim, Invoke or Renew. An
acknowledged in-memory action lease can also verify its exact original fallback
after autonomous expiry. A crash-recovered preclaim receipt lacks that later
acknowledgement: if the route changed and ownership already expired, public
fallback state cannot authenticate its history, so recovery remains pending.
Foreign owners and independently changed values remain preserved.

Focused native tests cover USB and CAN, atomic prior capture after preparation, ordered Boolean/integer/enum/UTF-8 arguments, one-shot endpoint aliases, external edits, lost replies, owner/token/route substitution, stale boot/configuration, bounded receipts and cleanup-only OS-protected roundtrips. These tests establish software protocol behavior; physical hardware qualification remains separate.

All 21 focused lease tests pass. The source-stamped native lease gate covers
880/918 production lines (95.86%), above the existing 71% minimum. The combined
30-case publication suite also exercises actual Host input-to-Setter delivery,
action cleanup before descriptor changes and autonomous expiry after an advanced
CAN delivery lease. See [publication composition](profile-publication.md).
