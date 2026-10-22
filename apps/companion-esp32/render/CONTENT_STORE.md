# Bounded renderer content

The allocation-free `content_store` and structural `package` modules are reused
from render-package work at commit `f2c9147b863a1aab7de1dd9026bf54e5430b16c4`.
They are target data infrastructure for feature #112. They do not introduce
another authoring format or replace the shared widget definition/instance model.
The prior custom scene compositor has not been imported.

The store streams at most 2048 bytes per write/hash operation into one of two
slots. Erasing advances one sector per call. Exact manifest and object SHA-256
digests are verified before an alternating journal commits its marker last.
Readers pin immutable views; a subsequent update cannot erase a slot while a
renderer still consumes it. A failed or interrupted update preserves a complete
older candidate or returns no valid content. Startup scanning verifies content
again; cached bytes never restore input authority or an armed session.

Manifest and content validation callbacks are mandatory. They must independently
check source publication identity, semantic descriptors, references, supported
operations and target resource limits. Passing structural checks or digest
checks supplies no input/session authority. The binary inspector uses reads of
at most 32 bytes, rejects noncanonical directory layout/overlap/reserved fields,
and bounds tables before exposing them to the scene decoder.

The portable fault-model tests exercise 318 interruptions across erase, streaming
write, hash, validation and journal commit boundaries, including partial writes.
They use actual SHA-256 and check that pinned readers remain valid after a swap.
Run `python3 apps/companion-esp32/test/content_store.py` and `test/package.py`
from the repository root with GCC, gcov and OpenSSL development headers. Both
run address/undefined-behavior sanitizers and enforce 71% coverage thresholds.

At this checkpoint the actual ESP32 partition adapter, compiled source admission
and authenticated USB/WSS package delivery are not connected. The existing
partition table and running main loop still use the legacy image cache/raster
path. No physical flash, NVS or device is written by these tests. Target package
limits in the reused inspector are provisional software bounds, not measured
LVGL board qualification.
