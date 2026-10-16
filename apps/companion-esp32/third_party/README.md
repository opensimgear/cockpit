# Third-party source

cjson/cJSON.c, cjson/cJSON.h and cjson/LICENSE are copied unmodified from the ESP-IDF v5.5.2 cJSON submodule at commit c859b25da02955fef659d658b8f324b5cde87be3 (Dave Gamble and cJSON contributors). The MIT permission/copyright notice is retained in each source and LICENSE. This same pinned source is compiled in firmware and native tests; parsing is configured with nesting limit 24. No other graphics/network library is vendored here.

ESP-IDF framework libraries retain their upstream licenses. The firmware uses ESP-IDF v5.5.2 TCP/TLS/WebSocket transport APIs directly; no managed WebSocket component is required. The existing repository licensing policy applies only to original OpenSimGear material.
