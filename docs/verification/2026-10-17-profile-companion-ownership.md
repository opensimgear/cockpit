# Companion assignment and feedback ownership

Profile assignments now carry a protected native run UUID, monotonically increasing takeover revision, and original-baseline digest/publication pin. A strict compact journal receipt captures immutable assignment references and inherits the original baseline only from the exact outgoing owner. Full source definitions are reconstructed and verified from retained publications under the Hub lock. Acquisition disarms the selected instance even when the panel hash is unchanged. Restoring a stale receipt preserves a newer owner; restart recovery can restore the exact owned baseline but cannot acquire or replay a profile panel.

Legacy assignment compare-and-swap and every legacy feedback configure/token/publish/clear path reject profile-owned instances. The profile feedback API checks the owned receipt plus actual source session, generation and feedback generation under the same Hub lock. Feedback delivery requires a real fresh companion baseline. Explicit operator reassignment revokes the profile stamp and cannot be undone by its stale receipt. Other companion instances retain their input authority.

Verification:

- Nine focused profile tests pass, including same-panel takeover, inherited baseline, peer isolation, old cleanup, missing fresh baseline, durable restart, failed storage replacement, operator revocation and tampered recovery. The maximum 128-control publication uses compact receipts below the journal's 8KiB limit and retains its original baseline through pruning and restart.
- All 82 companion service tests and nine native companion-host tests pass.
- `scripts/companion-coverage.py --output-dir .research/profile-companion-ownership-coverage` passes all 14 modules at the unchanged 71% minimum: owned receipts 309/316 (97.78%), feedback boundary 170/172 (98.84%), native metadata 356/477 (74.63%). The counter parser includes LLVM's abbreviated hot counts, preserving the full production-line denominator.

Live profile Host subscription and feedback adaptation remain a subsequent unit. These tests do not qualify physical transport, renderer behavior, or a game integration.

Recovery follow-up: protected owner matching is separate from retained publication validation. A missing panel cache leaves the exact owned recovery intent intact and returns an error; it is not reported as lost ownership. Neutralization may clear only the still-owned feedback book without a panel cache, while configuration/publication still require complete verified metadata and an actual source baseline. The restart regression covers missing both panels, restoring only the baseline, and successful restoration only after both immutable descriptors are available. All 83 portable service tests pass; `.research/profile-companion-recovery-coverage` passes all 13 service modules (owned recovery 328/336, 97.62%).
