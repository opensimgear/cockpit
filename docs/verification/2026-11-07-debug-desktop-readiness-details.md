# Debug desktop readiness follows the open details

The running native Cockpit opened a `profiles.desktop` reader on the Runtime &
recovery tab while Native runtime and desktop output details was collapsed.
The added regression observed one subscription where it expected zero.

The reader now follows the disclosure as well as the existing tab and
document visibility. Opening the disclosure while hidden defers the reader.
On reveal it receives the current readiness baseline. Collapse, hide and
navigation release the exact subscription, and reopening acquires a fresh
token. The existing manual detailed status command remains available.

The focused running Windows WebView test passed with four distinct readers
and no `profile_desktop_status` calls. The full navigation fixture passed on
the Windows WebView and in browser-dev. Both traces verified that expanding
adds only one desktop reader and collapsing retires it while the recovery
reader stays active.

The frontend was based on `e3fbc053` with the disclosure fix. Native used the
dev Cockpit executable with SHA-256
`0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`.
Browser-dev used the directly linked gateway with SHA-256
`2272c6b0035edb1470da3155b733c9b3a6bbccd14d996660a41b353fdba66242`.
All 1,706 frontend tests, Svelte checking and stream coverage passed.
The broader background and reopen matrix remains in #196.
