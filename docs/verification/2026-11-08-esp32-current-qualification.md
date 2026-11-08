# Current Sunton and Waveshare firmware builds

Both companion board images compiled from the Git archive of Cockpit
`2ae30263b7aeb86e46cc74d1650d0691d1d75347`. The private WSL build used ESP-IDF
v5.5.2 at `30aaf64524299d3bde422ca9a2848090d1bc5d0f` and Xtensa GCC 14.2.0.
GitHub timed out on the tool download. Espressif's mirror supplied the
same pinned assets, which the SDK installer verified before extraction.
[ESP-IDF release](https://github.com/espressif/esp-idf/releases/tag/v5.5.2)
and [Espressif mirror instructions](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-guides/tools/idf-tools.html).

| Board | Application bytes | Application SHA-256 |
| --- | ---: | --- |
| Sunton ESP32-3248S035R | 1,652,304 | `509cb089bd1e2ab4a1263ca4ad4a7e42b77238626ca4516c59a3f62badc7147d` |
| Waveshare ESP32-S3 Touch LCD 7 | 1,667,024 | `e8bfc8113661b5635ce3655d968e4d9f93c55dabb7095fe33f7543dd37633211` |

Both images fit the 2 MiB application partition. The partition-table SHA-256
is `599048fa47885e74561228921092f655eb26c40b82fe86304813c5349ec66291` on both
boards. The local binaries, configurations and logs are under
`.research/reactive/esp32-current-activation/`.

The Sunton and Waveshare driver suites and the portable rendering, networking,
storage, workspace, serial, render package, content store, widget source,
widget state, panel source, installer packaging and provisioning suites
passed. All seven generated fixture checks and four panel-export tests
passed. Panel-export coverage was 96.83% lines, 95.59% branches and 92.86%
functions.

The first whole-panel LVGL run reached catalogue case 6, which requires
37,419 bytes of JSON input. Its test harness was still using the default
32,768-byte reply limit. The corrected harness uses the generated rich
profile's limits for the shared catalogue and the per-target limits for the
compatibility cases. It also installs the bounded JSON allocator while it
compiles panels. The rich and compact renderer runs passed, along with the
firmware/LVGL software-client sanitizer smoke. Whole-panel compositor
coverage was 100% lines, 84.31% branches and 100% functions.

Physical touch, display, USB and Wi-Fi qualification remain in #199.
Sunton GPIO36 touch wake still needs a decision about Wi-Fi power saving.
