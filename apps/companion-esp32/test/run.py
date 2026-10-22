"""Native firmware checks, coverage gates and a PNG preview. Requires GCC/gcov."""
from pathlib import Path
import gzip
import json
import os
import struct
import subprocess
import zlib

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/companion-esp32"
OUT = ROOT / ".research/esp32-native"
OUT.mkdir(parents=True, exist_ok=True)
# Delete only this harness's old coverage counters, never source/build trees.
for counter in OUT.glob("core-test-*.gcda"):
    counter.unlink()
sources = [APP / p for p in ["core/companion.c", "core/widget.c", "core/raster.c", "render/widget_state.c", "render/widget_source.c", "network/input.c", "network/publication.c", "network/panel_source.c", "network/json.c", "network/workspace.c", "third_party/cjson/cJSON.c", "test/component_test.c", "test/core_test.c", "boards/esp_lcd_bridge.c"]]
subprocess.run([os.environ.get("CC", "gcc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic", "-g",
                "-fsanitize=address,undefined", "--coverage", "-DCJSON_NESTING_LIMIT=24", "-DCJSON_CIRCULAR_LIMIT=2048", *["-I" + str(APP / path) for path in ["core", "render", "network", "third_party/cjson", "test", "boards", "test/include"]],
                *map(str, sources), "-lm", "-lcrypto", "-o", str(OUT / "core-test")], check=True, cwd=ROOT)
subprocess.run([str(OUT / "core-test"), str(OUT / "preview.ppm"), str(OUT)], check=True, cwd=ROOT, env={**os.environ, "UBSAN_OPTIONS": "halt_on_error=1:print_stacktrace=1"})
subprocess.run(["gcov", "-j", "-b", "-c", "core-test-companion.gcno", "core-test-widget.gcno", "core-test-raster.gcno", "core-test-esp_lcd_bridge.gcno"], check=True, cwd=OUT)
report = {}
for name in ("companion", "widget", "raster", "esp_lcd_bridge"):
    data = json.loads(gzip.decompress((OUT / ("core-test-" + name + ".gcov.json.gz")).read_bytes()))
    file = next(f for f in data["files"] if f["file"].endswith("/" + name + ".c"))
    lines = file["lines"]
    branches = [b for line in lines for b in line.get("branches", [])]
    functions = file["functions"]
    metrics = {"lines": 100 * sum(l["count"] > 0 for l in lines) / len(lines),
               "branches": 100 * sum(b["count"] > 0 for b in branches) / len(branches),
               "functions": 100 * sum(f["execution_count"] > 0 for f in functions) / len(functions)}
    report[name] = metrics
    if any(value < 71 for value in metrics.values()):
        raise SystemExit(f"Coverage below 71%: {name}: {metrics}")
(OUT / "coverage.json").write_text(json.dumps(report, indent=2) + "\n")
# Convert the harness PPM using only the standard library.
magic, dimensions, maximum, rgb = (OUT / "preview.ppm").read_bytes().split(b"\n", 3)
assert magic == b"P6" and maximum == b"255"
width, height = map(int, dimensions.split())
def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
rows = b"".join(b"\0" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
(OUT / "preview.png").write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
print(json.dumps(report, indent=2))
print("Preview:", OUT / "preview.png")
for status in OUT.glob("status-*.ppm"):
    magic, dimensions, maximum, rgb = status.read_bytes().split(b"\n", 3)
    assert magic == b"P6" and maximum == b"255"
    width, height = map(int, dimensions.split())
    rows = b"".join(b"\0" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
    status.with_suffix(".png").write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
