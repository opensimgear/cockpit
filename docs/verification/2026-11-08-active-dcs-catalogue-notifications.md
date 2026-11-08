# Active DCS catalogue notifications

The active DCS reader now registers the same exact-file notifications as the
passive reader. The watched set includes the selected aircraft, aliases,
CommonData when applicable, and the metadata paths used by the decoder.
Registration is followed by a fresh content and file-metadata baseline before
the reader opens its transport. A changed file or lost watch invalidates that
reader and wakes the owning profile. The profile checks the watch before
accepting and again before publishing a capture. A replaced wake remains
bound to the same file scope, and Stop retires it.

The existing scheduled validation remains available even when registration
succeeds. The previous drive-alias probe showed that a Windows alias could
retarget without a directory notification. This turn's local NTFS hard-link
probe received the selected filename notification after a write through another
directory. That result covers the tested filesystem and operation. Independent
strict command checks and the scheduled validator remain in place.
[Drive-alias probe](2026-11-08-controller-visibility-wakes.md#volume-root-and-drive-aliases)

The native regression starts with live authored DCS values, writes a selected
catalogue file, observes the wake and withdraws the active reader without a
validation worker result. The passive regressions verify selected-file changes,
quiet sibling writes and rejection of a changed baseline after registration.

The live native comparison reproduced a pre-existing recovery problem. An
invalid alias document withdrew active samples while the profile owner stayed
applied. Restoring the original bytes recovered the passive inspector, but
the active profile remained stale. The same scenario failed on the preceding
`92f73a9c` build and on the watch candidate. Raw results are under
`.research/dcs-physics-native/1794162313947/` and
`.research/dcs-physics-native/1794162063221/`.

Recovery now prepares the returning reader from the retained metadata. The
previous reader's live tokens have already been revoked by this point. A
replacement that would discard all previously routed inputs or commands leaves
recovery pending. The inventory worker publishes the verified replacement
baseline against the exact predecessor and compiles a current delivery plan
with live tokens. Only the matching completion can install that plan. Active
delivery checks still require the returning owner to establish its own fresh
baseline, and commands still require a current invocation token.

The final running Cockpit fixture passed on browser-dev and native desktop.
Each transport withdrew all selected ownship and BIOS values on the invalid
alias document, recovered current values under the same applied owner after
restoring the file, and withdrew the values again on the confirmed process
exit. Both runs reported clean teardown. The authored results are under
`.research/dcs-physics-native/1794163393574/` and
`.research/dcs-physics-native/1794163516498/`. Real simulator and physical
input latency acceptance remain tracked in #199.

The final Windows suite passed 2,473 tests with 18 default ignores. The
existing STM32 deadline fixture passed separately on retry, and both
checksum-pinned DCS archive tests passed separately. The 31 focused tests
passed. Production line coverage across the six changed modules was
79.25–98.15%, with function coverage of 73.91–93.75%. The isolated build used
the Rust library output and disabled local debug symbols to fit the available
disk space. The tracked Cargo manifest is untouched.
