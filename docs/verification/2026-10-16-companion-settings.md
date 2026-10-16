# Companion settings and pairing candidates

Clicking a companion tile opens its settings: persisted name, panel assignment, and USB/Wi-Fi connection settings for ESP32 devices. The separate reset/re-pair action has been removed. Removing a companion revokes its host credential; the USB device then becomes eligible for pairing again, which overwrites its old setup through the normal provisioning flow.

USB pairing candidates exclude known paired identities, including Wi-Fi companions attached by USB. A fresh nonce-bearing status probe identifies inactive ports; the already connected transport supplies its current identity without opening the port twice. Settings lists only the selected device. Native firmware installation and new provisioning also reject a device still paired to the host.

Editing an ESP32 connection requires the selected paired screen on USB and preserves its companion ID and existing assignment. Changing only a name does not rotate its credential or disconnect its transport. Feedback remains a toast.

Validation covers candidate visibility, saved identity probing, rename identity/credential preservation, native builds and Svelte checks. Windows UI checks cover tile settings and exclusion of the connected paired Waveshare from new-pair candidates.
