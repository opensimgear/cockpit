# Windows production HidHide session checks — 15 October 2026

#26 remains In progress, with **7/11 criteria verified**. Ordinary production
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
Criterion 11 is partial: both inverse observers passed, but both identities have
not yet been measured during normal mode. Criteria 1, 6 and 8 remain open: the
complete setup matrix, combined game/vJoy routing and all failure cases are not
claimed. Reconnect, executable relocation, owner crash, feeder loss, external
conflicts and persistent recovery need further production qualification.

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
