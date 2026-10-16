"""UART console buffer startup checks; GCC/gcov, no physical port access."""
from pathlib import Path
import gzip
import json
import os
import subprocess
ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/companion-esp32"
OUT = ROOT / ".research/esp32-serial"
OUT.mkdir(parents=True, exist_ok=True)
for counter in OUT.glob("serial-test-*.gcda"):
    counter.unlink()
flags = [os.environ.get("CC", "gcc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic",
         "-g", "-fsanitize=address,undefined", "--coverage"]
flags += ["-I" + str(APP / folder) for folder in ("test/serial_include", "network", "core", "third_party/cjson")]
sources = [str(APP / "network/serial.c"), str(APP / "test/serial_test.c")]
for uart, baud, binary in ((1, 921600, "serial-test"), (1, 115200, "sunton-serial-test"), (0, 921600, "usb-test")):
    subprocess.run([*flags, "-DCONFIG_ESP_CONSOLE_UART=" + str(uart), "-DCONFIG_OSG_SERIAL_BAUD=" + str(baud), *sources, "-o", str(OUT / binary)], check=True)
    subprocess.run([str(OUT / binary)], check=True)
subprocess.run(["gcov", "-j", "-b", "-c", "serial-test-serial.gcno"], check=True, cwd=OUT)
data = json.loads(gzip.decompress((OUT / "serial-test-serial.gcov.json.gz").read_bytes()))
file = next(f for f in data["files"] if f["file"].endswith("/network/serial.c"))
lines, functions = file["lines"], file["functions"]
branches = [b for line in lines for b in line.get("branches", [])]
metrics = {"lines": 100 * sum(line["count"] > 0 for line in lines) / len(lines),
           "branches": 100 * sum(branch["count"] > 0 for branch in branches) / len(branches),
           "functions": 100 * sum(f["execution_count"] > 0 for f in functions) / len(functions)}
(OUT / "coverage.json").write_text(json.dumps(metrics, indent=2) + "\n")
print(json.dumps(metrics, indent=2))
if any(value < 71 for value in metrics.values()):
    raise SystemExit("Serial driver coverage below 71%")
