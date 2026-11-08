# Direct USB detail reader lifetime

Leaving Devices already unmounts its detail card. Hiding the document could
leave the mounted Direct USB card's exact input and metadata readers running.
Both readers now use the shared document visibility lifecycle. They retire
on hide, clear the old input and metadata, and reopen on current baselines.
The card keeps its setup draft and waits for the metadata baseline before
enabling controls again. Opening a new card makes one passive status read.
Revealing the mounted card uses its streams.

The component regression starts hidden, opens both readers, preserves an
unsaved draft through hide, then reconciles a new connection generation
without keeping stale setup authority.

Both running Cockpit clients opened the card from Devices, navigated away,
reopened the card, then hid and revealed the document. Each client acquired
and retired six exact readers and made two status reads, one per card mount.
The rendered axis followed the authored values 14,000, 24,000 and 34,000.
The fixture authored USB replies at the transport boundary, with the actual
Tauri and browser observation adapters handling delivery and acknowledgement.
Other commands and streams used private running Cockpit sessions.

All 49 USB tests, 131 system inventory tests and their coverage gates passed.
Each configured module exceeded 90% in lines, branches and functions.
Svelte reported zero errors and warnings. The production frontend build
passed with the bundle size warning.

The native executable SHA-256 was
`dcc8ccd4a78c47d4f86d0f0d28329d50843c39fefa727317c1519f0a6cb92546`.
The browser-dev gateway SHA-256 was
`2cd89706b01e0b37f29007996d118827dc5e613538c18b804a3ff1207747ac7b`.
Physical USB qualification remains in #199.
