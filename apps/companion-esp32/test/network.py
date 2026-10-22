"""Native network protocol/client tests. GCC/gcov, Python 3 and libssl-dev required."""
from pathlib import Path
import gzip
import json
import os
import subprocess
ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/companion-esp32"
OUT = ROOT / ".research/esp32-network"
OUT.mkdir(parents=True, exist_ok=True)
for pattern in ("wire-test-*.gcda", "client-test-*.gcda", "input-test-*.gcda", "serial-wire-test-*.gcda", "pixels-test-*.gcda"):
    for counter in OUT.glob(pattern):
        counter.unlink()
flags = [os.environ.get("CC", "gcc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic", "-g", "-fsanitize=address,undefined", "--coverage", "-DCJSON_NESTING_LIMIT=24", "-DCJSON_CIRCULAR_LIMIT=2048"]
includes = ["test/include", "core", "network", "render", "third_party/cjson", "test"]
flags += ["-I" + str(APP / p) for p in includes]
environment = {**os.environ, "UBSAN_OPTIONS": "halt_on_error=1:print_stacktrace=1"}
common = ["core/companion.c", "core/widget.c", "render/widget_state.c", "render/widget_source.c", "network/input.c", "network/json.c", "network/workspace.c", "network/publication.c", "network/panel_source.c", "network/scene_bank.c", "network/settings.c", "third_party/cjson/cJSON.c"]
for binary, test in (("wire-test", "wire_test.c"), ("client-test", "client_test.c"), ("input-test", "input_test.c"), ("serial-wire-test", "serial_wire_test.c")):
    serial = [str(APP / "network/serial_json.c")] if binary in ("client-test", "serial-wire-test") else []
    subprocess.run([*flags, *[str(APP / p) for p in common], *serial, str(APP / "test" / test), "-lm", "-lcrypto", "-o", str(OUT / binary)], check=True, cwd=ROOT)
    with (OUT / (binary + ".log")).open("w") as log:
        subprocess.run([str(OUT / binary), str(APP / "test/live-publication.json")], check=True, cwd=ROOT, stdout=log, env=environment)
    print(binary, "passed")
subprocess.run([*flags, str(APP / "test/pixels_test.c"), "-o", str(OUT / "pixels-test")], check=True, cwd=ROOT)
subprocess.run([str(OUT / "pixels-test")], check=True, cwd=ROOT, env=environment)
subprocess.run(["gcov", "-j", "-b", "-c", "wire-test-json.gcno", "wire-test-publication.gcno", "wire-test-settings.gcno", "client-test-client_test.gcno", "serial-wire-test-serial_json.gcno", "input-test-input.gcno", "pixels-test-pixels_test.gcno"], check=True, cwd=OUT, stdout=subprocess.DEVNULL)
report = {}
for module, artifact in (("json", "wire-test-json"), ("publication", "wire-test-publication"), ("settings", "wire-test-settings"), ("client", "client-test-client_test"), ("serial_json", "serial-wire-test-serial_json"), ("input", "input-test-input"), ("pixels", "pixels-test-pixels_test")):
    data = json.loads(gzip.decompress((OUT / (artifact + ".gcov.json.gz")).read_bytes()))
    source = next(f for f in data["files"] if f["file"].replace("/../", "/").endswith("/network/" + module + ".c"))
    lines, functions = source["lines"], source["functions"]
    branches = [b for line in lines for b in line.get("branches", [])]
    metrics = {"lines": 100 * sum(l["count"] > 0 for l in lines) / len(lines), "branches": 100 * sum(b["count"] > 0 for b in branches) / len(branches), "functions": 100 * sum(f["execution_count"] > 0 for f in functions) / len(functions)}
    report[module] = metrics
(OUT / "coverage.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
for module, metrics in report.items():
    if any(value < 71 for value in metrics.values()):
        raise SystemExit(f"Coverage below 71%: {module}: {metrics}")
