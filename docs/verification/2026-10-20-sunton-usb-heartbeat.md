# Sunton USB heartbeat framing

The running development Cockpit reported the paired Sunton on COM8 as USB-connected, with its panel session disconnected and operation attempt 83 failed: `Heartbeat: Invalid USB reply JSON` (`request-failed`). Its image preparation had completed using one cache hit and no downloads. An explicit reconnect restored the panel, confirming that USB enumeration and saved pairing were still available.

Firmware used separate newline accumulators for idle console commands and in-flight protocol replies. A periodic status probe could arrive only partly before the network task started a heartbeat. The reply reader then parsed the remaining suffix as a complete JSON message. This explains how a valid host message could produce the reported failure, especially on the Sunton's 115200-baud link.

Firmware `2026.10.20.2` uses one persistent bounded decoder for both readers. Partial records survive reader handoffs and request timeouts; completed records clear their storage. Late protocol replies are discarded by idle command handling. The existing 8192-byte record limit, reply identity checks, request deadlines and malformed-record failures remain enforced. The shared decoder replaces the old console buffer without adding another full-size buffer. Pairing, partition layout and image cache format are unchanged.

The native writer has one owner per serial port and writes complete JSON records followed by newline. Direct reply envelopes fit below 4.3 KiB and chunk envelopes below 3.1 KiB, within the existing firmware bound. No native transport change is required.

## Verification

The serial regression exercises every split inside a status request before a heartbeat, reverse handoff of a reply interrupted by its deadline, overflow across a handoff and recovery with a subsequent valid reply. Existing deferred-command, status responsiveness, malformed reply, identity and checksum cases remain in the same harness.

- The unchanged previous reader fails the new fragmented-status regression at its first split. The final reader passes every tested split, timeout and overflow case.
- `python3 apps/companion-esp32/test/network.py` passed under WSL with AddressSanitizer and UndefinedBehaviorSanitizer: wire, client, input, serial wire and pixel tests. All existing coverage gates pass. The decoder has 100% line/function coverage and 83.33% branch coverage.
- Sunton and Waveshare builds passed with pinned ESP-IDF v5.5.2. Applications occupy 892,400 and 908,848 bytes, respectively, within the existing 1.5 MiB allocation. Both partition tables retain SHA-256 `4c371bf1aef93f2aed6f99bec8b6f8e16c6b71f47e7fef6d6480c6b76591f369`; bootloaders are unchanged.
- The rebuilt native development Cockpit installed and verified `2026.10.20.2` on the paired Sunton on COM8 using its setup-preserving update flow. The first attempt required bootloader entry; the existing standard DTR/RTS reset helper successfully entered download mode, after which installation completed. No manual button operation was needed.

- `node --test tests/ui/companion-image-cache-native.mjs` passed against the actual running Cockpit and physical Sunton in 23.7 seconds. Five panel switches reused cached images with no downloads (2.39–2.92 seconds each); explicit stop and fresh reconnect also passed. The original assignment was restored and temporary panels removed.
- `node --test tests/ui/companion-serial-heartbeat-native.mjs` passed in 92.1 seconds. After one explicit reconnect, 551 observations over 90,099 ms retained the same armed USB session with no failed operation or session loss. Full nonce-bound status remained at most 1,037 ms old. All companion assignments remained unchanged.

Local evidence is retained under `.research/sunton-heartbeat/`, `.research/esp32-network/`, `.research/companion-image-cache-native/` and `.research/companion-serial-heartbeat-native/`. These checks cover deterministic fragmentation and the attached USB Sunton; they do not qualify arbitrary cable failures or physical Wi-Fi behavior.
