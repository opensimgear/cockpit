# Windows production HidHide session checks — 15 October 2026

#26 remains In progress, with **8/11 criteria verified**. Ordinary production
qualification gates stay false. Tests use a separate explicit host and local state.

## Native ABI correction

Read-only plans correctly rejected changes against the existing three hidden
devices and four application entries. The user approved temporarily clearing only
the hidden entries, preserving application entries and flags. The exact baseline
was saved before an independent guardian tracked the host by PID, creation time
and executable path.

The first write cleared the list but the adapter rejected its acknowledgement.
The pinned release's `OnControlDeviceIoSetWhitelist/SetBlacklist` completes with
`inputBufferLength`, despite having no output payload. Empty collection reads
return a single terminating WCHAR (two bytes), also rejected by the old adapter.
Further operations were paused. A corrected bounded helper restored the exact
saved policy and the independent M0 verifier confirmed restoration. That failed
attempt is not a passing session.

The production adapter now distinguishes list-write acknowledgement size from
response payload size and accepts the bounded two-byte empty collection. Boolean
response lengths, operation directions, malformed/oversized frames and inconsistent
counts remain checked. A regression clearing/restoring complete policy through the
native transport translation failed before correction. All **69 local Windows
controller-input tests** then passed, including two new transport regressions.
The production mutation gate remains false.

## Installed-driver session observations

A fresh isolated host used the corrected production transport, service requests,
source admission, executable/instance resolution, planner, worker and durable
journal. Only scratch qualification gates and a read-only source accessor differ;
no algorithm or permission check is bypassed. The test guardian can use production
journal recovery after host exit, then restore the authorized setup if no external
conflict exists. It is test infrastructure, not automatic normal-app crash cleanup.

The second setup succeeded. The source was VKBsim Gladiator EVO R (231D:0200),
firmware v2.20C, direct USB. Fresh observer processes avoid existing-handle ambiguity.

| Phase | Direct HID open | DirectInput enumeration | Production capture |
| --- | --- | --- | --- |
| Before hiding | Succeeds | Original present | Available |
| Normal hide-all, observer | Access denied, error 5 | Original absent | Fresh reports; user-held trigger observed as 1 |
| After production Stop/Restore | Succeeds | Original present | Available |
| Inverse, selected `observer.exe` | Access denied, error 5 | Original absent | Fresh-report admission passed |
| Inverse, separate unselected executable | Succeeds | Original present | Same session active |

Normal and inverse Stop/Restore completed with clean journals and no required
recovery. External application entries survived production cleanup. The setup
finisher then restored all original application/device entries and flags;
independent verification confirmed an exact match. No vJoy slot changed.

**Raw Input remains visible to the blocked observer.** These results establish only
the tested direct-HID/DirectInput behavior, not universal application input hiding.
Gates remain disabled while scope and outstanding qualification remain open.

## Acceptance and provenance

Previously verified criteria 2, 5, 7 and 10 remain checked. Criterion 3 now has
native hide-all/capture evidence; criterion 4 has actual selected/unselected process
evidence; criterion 9 has production cleanup and exact external setup restoration.
Criterion 11 is now verified: fresh selected and unselected observers also both
reported direct-HID access denied (error 5) and no original DirectInput device
during normal mode. Criteria 1, 6 and 8 remain open: the
complete setup matrix, combined game/vJoy routing and all failure cases are not
claimed. The setup matrix, combined feeder loss and remaining external-conflict
cases need further production qualification. Later fault checks are recorded below.

## Source reconnect — 16 October 2026

The first bounded monitor expired before the user's reconnect reply. Its guardian
restored the exact original policy, but that run does not establish source-loss
behavior. A fresh synchronized retry used session
`ff74acd6-2f4d-4d6c-8836-276db43f2167`, a separate setup guardian and continuous
status monitoring. The user unplugged only the right Gladiator for five seconds
and reconnected it to the same port with games/feeders closed.

The monitor recorded the active session becoming inactive with phase `attention`
and message “Selected controller requires a complete current baseline. Visibility
restored.” The production journal was clean, position zero and no pending change.
After reconnection the same source ID had a complete fresh capture baseline;
the job remained inactive. A new observer opened the original HID successfully
and enumerated one matching DirectInput device. Reconnection did not reapply
hiding. The setup finisher and independent M0 verifier confirmed exact restoration
of the original three hidden entries, four applications and both flags. No vJoy
slot changed. Criterion 8 stays unchecked until its other named cases pass.

## Executable relocation and session-owner crash — 16 October 2026

An inverse session selected the temporary observer executable. Renaming that
file caused the production worker to stop with “Choose an existing absolute
executable path. Visibility restored.” The journal was clean. The executable
was moved back and the original external policy was independently verified.

A separate hide-all session (`d866fc18-3886-4d60-bcae-c48058a2d388`) was terminated
abruptly by exact PID and executable identity. The restarted service reported
`recovery.required=true` with the recorded session ID and original device instance.
Its production Restore request completed with “Owned visibility changes restored
and verified”, a clean journal and `recovery.required=false`. The setup finisher
restored the external baseline, independently verified. This demonstrates explicit
recovery after a persistent-adapter owner crash; it does not claim automatic
normal-app cleanup. The test-only guardian allowed a bounded 30-second recovery
window before its emergency restore; the restarted service completed recovery
within that window, before the guardian performed any restore.

Criterion 8 still needs setup-failure and combined feeder-loss evidence. These
additional passing cases do not complete the composite criterion. All ordinary
capability gates remain disabled.

The first CI attempt for `1ffc752` passed controller-input tests but failed a
mapping-host script lifecycle test when its safety heartbeat expired. The failed
Windows job was rerun; its result must be checked before claiming full CI success.

Machine data remains in ignored `.research/review-audit/` storage. SHA-256:

| Evidence | SHA-256 |
| --- | --- |
| `visibility-normal-before-observer.json` | `c71393734743b57608d47f422670359e68b78649b4fc62a9244301834abce65a` |
| `visibility-normal-hidden-observer.json` | `e1f981da1dce0854611313c5a3abff69757b933e1cb848c2392afe9f619115c2` |
| `visibility-normal-held-snapshot.json` | `43c320c271a799ce196d1eb51fb58925c1c237f3fa636e6f6edecee7fecf4cdb` |
| `visibility-normal-clean-journal.json` | `572721be5e7a8c65e886024a8499e1ca98a918180de3856c3caa7f3f44ee7c7f` |
| `visibility-inverse-selected-observer.json` | `56a41837d16d425d7a16cfd309f870b28df69fcb3a426a9c245ae00d50da3e81` |
| `visibility-inverse-unselected-observer.json` | `329ac3c0e80f3d43d044844dd1f26fe9a27106bb91175b45ff360339389d4f58` |
| `visibility-inverse-clean-journal.json` | `732982f6945ae5a802466dc71dea8e0acb3113932cc33f7686f8e79f06c673b1` |
| `visibility-fixed1-setup-restored.json` | `0daa6b48be1a25948b06cd46355d94ea12251452ffa1c147067f22feea56a1c` |
| `visibility-normal-selected-complete.json` | `a744998b2e133e29111496f55c0f7ef8fbf5ff93a74232a2ac371240f8e80eb8` |
| `visibility-normal-unselected-complete.json` | `6128651333683018fac37627ebe39e9361c40661482079ce62c3f1c6782c438e` |
| `visibility-retry1-monitor.jsonl` | `0197bac3ab6ce4ec4b6addc96630e0c8cb89c2dc65feb1c87a8170acf99bc010` |
| `visibility-retry1-clean-journal.json` | `530c51937ea215a0dc9c988e1cd062360fc36b835432bcc523a23ca84bf35ac7` |
| `visibility-retry1-reconnected-observer.json` | `ca2450896e7f3a3ad2c96701a0bf3b955ced679e050b79625681d3b53d3f5dd1` |
| `visibility-retry1-reconnected-snapshot.json` | `227e4e1464214ce4323a512ef4aeaa6d1e9cdd6856f30e1c6344044a871bb8a6` |
| `visibility-retry1-reconnected-status.json` | `c1f55fe2ac59daad99c8ee1a6093d22e2426d93275f6a71864cd14d21ad27283` |
| `visibility-retry1-finish-setup.json` | `cb2ac7de9b66689d629a4557fae0c5ee495467bc1ed63cb53e9a9e74cd00fdb8` |
| `visibility-relocation1-after-status.json` | `70e0fd271c30e37c2ccbc4050afe8cec99a3ffcda5979e21cb28a586bbef0bdf` |
| `visibility-relocation1-clean-journal.json` | `824db485c4c93f028db34490035386041f17060bd4082e9d1bd8dc1c7c5f0f27` |
| `visibility-owner-crash1-restarted-status.json` | `928207a03c702e7ac24fa0b6ce5a3e1b3403f622134fe0380a6ae2ad929bdc3c` |
| `visibility-owner-crash1-recovered-status.json` | `ee6cf0581fd73610b60610aa143ef0a9a6b20e55a80c5709f92a320dd6cac23e` |
| `visibility-owner-crash1-clean-journal.json` | `9de3b872fded6d1e81aced822e45a82d817e19f066470310ef4ed62b2770059d` |
