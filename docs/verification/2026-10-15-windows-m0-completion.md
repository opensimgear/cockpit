# Windows M0 continuation and contract baseline — 15 October 2026

All eight listed Windows #21 criteria now have evidence for the bounded contract
in [controller-mapping-contracts.md](../controller-mapping-contracts.md). This is
dependency/protocol qualification, not production enablement or a claim of complete
Windows mapping delivery. #26/#31/#33 retain the deployment and feature checks;
Linux/macOS work is deferred to #48/#49. All production gates remain unchanged.

## Preserving the current custom configuration

The user closed games and feeders and approved each elevated isolated probe.
The old implicit-only/FFB reconfiguration suite was not rerun: it assumes no
persistent descriptors and would not preserve the three current slots.

A separate bounded qualification executable calls the production provisioning
planner, protected native journal, SYSTEM-hive transaction, descriptor writer,
ownership-checked removal and driver refresher. A wrapper injects failure or
cooperative cancellation immediately after a successful native slot-4 write and
before its first publication refresh. It may create/remove **only slot 4**; its
plan and fresh preflight require the unchanged free slots 1/2/3 and no pending
recovery. Production rollback, rather than custom cleanup, restores the baseline.

| Case | Observed result |
| --- | --- |
| Native post-write failure | `Injected post-write native failure Original configuration restored.` |
| Native post-write cancellation | `Injected cooperative cancellation after native write Original configuration restored.` |
| Persistent slots 1/2/3 | Exact before/after configuration and published capabilities match; all slots free |
| Temporary slot 4 | Native write recorded, only owned slot removed, restoration refresh passed |
| OEM/ancillary fingerprint | Unchanged: `51b2a928c1cb59ffece976fa445d46cc2ce1eeebaa59c1564c959c64161a6bde` |
| Recovery | No pending operation after either run; durable ownership retired |
| HidHide | Fresh verification matches the exact existing normal policy |

Slot 1 retains 16 axes/eight buttons/no hats; slot 2 retains X/Y/eight buttons/one
continuous hat; slot 3 retains X/Y/Z/sixteen buttons/two continuous hats. All are
non-FFB. Successful persistent custom creation, retention and refresh were already
recorded during [M3 qualification](2026-10-15-vjoy-m3-windows.md).

These are native registry/driver checks with qualified injection timing. They do
not claim cancellation of an in-flight PnP operation or real force-feedback output.
If a refresh is uncertain or needs reboot, the contract retains recovery evidence
rather than starting a second operation or claiming runtime restoration.

## FFB support boundary and prior recovery

The earlier M0 experiment loaded a 12-button FFB layout, then hit a real Windows
restart gate. Its incomplete continuous-restart suite remains historical failure
evidence, not a successful FFB reconfiguration test. After the user's reboot,
independent verification restored the original eight-button/non-FFB slot and
unchanged OEM registration/HidHide policy; the original boot/restoration record is
retained. The subsequent production preflight observed the FFB runtime and refused
provisioning before elevation/mutation (`create2-fixed.jsonl`).

The supported contract deliberately preserves FFB/discrete-POV/unknown layouts by
rejecting provisioning, and never creates or rewrites them. Existing custom
non-FFB layouts and OEM FFB registration are preserved through the successful,
failed and cancelled native paths above. This completes criterion 5 within that
explicit support boundary; it does not turn the earlier FFB suite into a pass.

## Other M0 evidence reconciled

The earlier normal-mode and inverse-mode HidHide specimens, injected exception
and forced-exit probes are retained. The inverse specimen had six journaled stages:
the capture executable retained direct-HID/DirectInput access while the selected
observer lost access; Raw Input remained visible. Normal completion, injected
failure and forced exit restored the exact original lists and flags through the
guardian. A fresh read-only verification after both new rollback checks again
reported `restored: true, exactPolicyMatch: true`. These experiments qualify only
the tested APIs; production sessions and game integration remain #26 work.

Other listed M0 criteria retain their existing evidence: optional-dependency
startup/independent readiness rejection, physical capture with ACR foreground,
complete vJoy reports, bounded infinite Rhai evaluation and recorded limitations.
Pinned driver/SDK/HidHide versions, signatures, hashes and V3 ABI are retained in
the [original M0 record](2026-10-14-controller-mapping-m0.md). No signing protection
was disabled. Windows contract v1 is now the implementation baseline; other
platforms and unrestricted visibility policies are not implicitly qualified.

## Validation and provenance

All 18 targeted Windows provisioning regressions passed, including preservation,
partial failures, ownership conflicts, cooperative cancellation and uncertain
refresh refusal. Existing source guards reject FFB/discrete-POV capability shapes.
Plan checkpoint `4f2d4fe` passed all 11 CI jobs in
[run 38786937187](https://github.com/opensimgear/cockpit/actions/runs/38786937187).
This continuation changes documentation only; native hardware observations remain
separate from automated CI. Raw machine evidence stays in ignored research storage.

| Evidence under `.research/review-audit/` | SHA-256 |
| --- | --- |
| `m0-preservation-failure-result.json` | `d94d2ec2f623da1dc061575e473421bb9a26d91e6c938c60761034f4380c4174` |
| `m0-preservation-cancel-result.json` | `96c07cd3e316a5a292cfbf4426d69635060fd8ed72dcfa1ba1f68c2b89d70950` |
| `m0-after-preservation-sdk.json` | `4fb6d72ec3ce3b2c7882a4ee002b75b32ebcb1a2adecc84aca86a1042b56e980` |
| `create2-fixed.jsonl` (historical FFB preflight) | `8f951d49cf6dd62d018afcdf8b933bf0d5a2bb9dc646e23bad629e33ec6c7670` |

Post-reboot evidence `windows-followup-after.json` SHA-256:
`1b60ccbc02f64ed18f0c92100a3bb502e0e78b495adfdf4862ffc341bd78c595`.
