# Companion roster visibility

The Companions screen releases its own presence and listener observers
when the document hides or the view closes. Its passive status read is
fenced by the visibility period that requested it. Pairing commands,
settings saves and active firmware operations keep their own lifetimes.
Other consumers, including the Devices inventory, can still use the
shared native topics.

## Running Windows Cockpit

The previous frontend issued a follow-up roster read while hidden after
its delayed status response was delivered. The fixture uses a private
listener on loopback and holds a passive reply from before hiding. An
authored marker in that delayed reply makes stale roster updates visible.
Native WebView and browser-dev kept the marker out of the roster, made
zero hidden status reads and each made two reads on reveal. Both clients
created a pairing link using the listener state committed while hidden,
then cancelled the temporary offer and disabled the private listener.
The marker was confined to the fixture response.

The existing Companion library and panel/widget draft reopen regressions
also passed on both transports. The live runs used the Windows binaries
identified in the [idle trace matrix](2026-11-08-idle-transport-matrix.md).
Local records are in `.research/reactive/companion-roster-visibility/`.

## Local checks

All 1,708 frontend tests, the Svelte check and the production frontend
build passed. The 19 focused tests passed the coverage floor for the
visibility, pairing and settings helpers. All three reached 100% line
and function coverage. Branch coverage was 100% for the visibility
helper, 84.21% for pairing and 95% for settings.
