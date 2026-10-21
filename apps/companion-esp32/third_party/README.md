# Third-party source

cjson/cJSON.c, cjson/cJSON.h and cjson/LICENSE are copied unmodified from the ESP-IDF v5.5.2 cJSON submodule at commit c859b25da02955fef659d658b8f324b5cde87be3 (Dave Gamble and cJSON contributors). The MIT permission/copyright notice is retained in each source and LICENSE. This same pinned source is compiled in firmware and native tests; parsing is configured with nesting limit 24. No other graphics/network library is vendored here.

ESP-IDF framework libraries retain their upstream licenses. The firmware uses ESP-IDF v5.5.2 TCP/TLS/WebSocket transport APIs directly; no managed WebSocket component is required. The existing repository licensing policy applies only to original OpenSimGear material.

LVGL 9.4.0 is downloaded unmodified from the exact upstream commit and SHA-256
archive recorded in [lvgl.lock.json](lvgl.lock.json). `tools/lvgl_source.py`
verifies that archive before extracting into the ignored `.research` cache.
The IDF component and native sanitizer harness share that source and
`render/lv_conf.h`. The archive retains LVGL's `LICENCE.txt` (MIT, copyright
LVGL Kft) and its transitive attribution in `COPYRIGHTS.md`. Neither file may be
removed from distributed source or notices when packaging the renderer.
