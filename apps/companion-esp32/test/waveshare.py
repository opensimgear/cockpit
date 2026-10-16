"""Waveshare adapter fault injection and per-production-file coverage; no hardware I/O."""
from pathlib import Path
import gzip
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/companion-esp32"
OUT = ROOT / ".research/esp32-waveshare-native"
OUT.mkdir(parents=True, exist_ok=True)
for counter in OUT.glob("board-test-*.gcda"):
    counter.unlink()
subprocess.run([os.environ.get("CC", "gcc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic",
                "-g", "-fsanitize=address,undefined", "--coverage",
                "-I" + str(APP / "test/board_include"), "-I" + str(APP / "boards"),
                "-I" + str(APP / "core"), str(APP / "test/waveshare_test.c"),
                "-o", str(OUT / "board-test")], check=True, cwd=ROOT)
result = subprocess.run([str(OUT / "board-test")], check=True, capture_output=True, text=True, cwd=ROOT)
(OUT / "board-test.log").write_text(result.stdout + result.stderr)
print(result.stdout, end="")
subprocess.run(["gcov", "-j", "-b", "-c", "board-test-waveshare_test.gcno"], check=True, cwd=OUT)
data = json.loads(gzip.decompress((OUT / "board-test-waveshare_test.gcov.json.gz").read_bytes()))
file = next(f for f in data["files"] if f["file"].endswith("/boards/waveshare-lcd7/board.c"))
lines, functions = file["lines"], file["functions"]
branches = [b for line in lines for b in line.get("branches", [])]
metrics = {"lines": 100 * sum(line["count"] > 0 for line in lines) / len(lines),
           "branches": 100 * sum(branch["count"] > 0 for branch in branches) / len(branches),
           "functions": 100 * sum(f["execution_count"] > 0 for f in functions) / len(functions)}
(OUT / "coverage.json").write_text(json.dumps({"waveshare-lcd7": metrics}, indent=2) + "\n")
print(json.dumps(metrics, indent=2))
if any(value < 71 for value in metrics.values()):
    raise SystemExit("Board adapter coverage below 71%")
