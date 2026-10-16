# Paired USB companion reconnection

Cockpit now starts a background USB detector when the native runtime opens. Existing protected pairing state is loaded on startup; first-time users do not initialize protected companion storage until requested.

The detector enumerates ESP32 USB bridges, sends a bounded status probe with a fresh nonce, and only opens the normal transport for a saved direct-USB installation whose companion identity is already paired with this host. Pairing remains the persisted identity authority; COM port numbers are never identities. The transport rechecks the identity after opening the port and authenticates with the board's saved credential.

A successful connection delivers the Cockpit-controlled assignment as usual. Unplugging closes the old connection and neutralizes its source. Detection retries for replugged devices, changed COM ports and temporary port failures. Status silence also releases stale serial handles. Explicit Release USB is respected for the rest of the app session; manual Connect or a Cockpit restart enables detection again. Flashing and provisioning keep exclusive ownership of their USB port.

Validation: seven USB transport/flashing/discovery tests passed, including saved/unknown/removed identities, nonce mismatch, non-ESP32 and Wi-Fi rejection, startup detection, changed COM ports, unplug/replug and probe commands restricted to status.

Windows hardware check: the already-paired Waveshare was detected after Cockpit restarted and its tile changed to Connected without pairing or provisioning. The existing no-panel assignment was preserved.
