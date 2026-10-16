# Multiple serial companions and baud discovery

- USB manager retains an independent worker per port. Selecting, flashing or releasing a target no longer shuts down all workers. Paired-device discovery skips ports already open and continues through all eligible ports.
- Device status and firmware metadata are collected across workers; companion details select the matching device identity.
- Regression test verifies COM5/COM6 command isolation, targeted release and full shutdown. Serial suite: 15 passed. Windows companion host suite: 32 passed. Svelte checks passed.
- Live follow-up found Sunton connected while Waveshare remained offline. Read-only COM5 requests initially got no status at either baud. A newline-prefixed 921600 request immediately returned the Waveshare identity, saved serial setup and firmware 2026.10.16.2.
- Discovery now terminates any partial record left by probing another baud before sending a nonce-bound status command. The active worker does the same when checking status. Native probe tests assert the leading delimiter.
- Updated Windows mirror and restarted Cockpit. Both Sunton 3.5in and ESP32 screen automatically reconnected with Panel 1 and existing pairing. Both tiles were still Connected after an additional 30-second observation. No board flash or re-pairing was required.
