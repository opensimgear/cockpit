"""SDK-free streaming render-object inspector, host ABI/goldens and coverage.

Only builds/runs native tests; no board, content store or firmware is accessed.
The generated host fixtures are checked in, so this runner needs GCC/Python,
not Node or ESP-IDF.
"""
from pathlib import Path
import gzip
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/companion-esp32"
OUT = ROOT / ".research/esp32-render-package-native"
FIXTURES = ROOT / "packages/companion-renderer/test/fixtures"
OUT.mkdir(parents=True, exist_ok=True)
for counter in OUT.glob("package-test-*.gcda"):
    counter.unlink()
subprocess.run([
    os.environ.get("CC", "gcc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic",
    "-g", "-fsanitize=address,undefined", "--coverage",
    str(APP / "render/package.c"), str(APP / "test/package_test.c"),
    "-o", str(OUT / "package-test"),
], check=True, cwd=ROOT)
fixture_paths = [FIXTURES / (kind + ".bin") for kind in ("semantics", "scene", "images", "fonts")]
missing = [str(path) for path in [FIXTURES / "abi.json", *fixture_paths] if not path.is_file()]
if missing:
    raise SystemExit("Generate host render-package fixtures first: " + ", ".join(missing))
expected_abi = json.loads((FIXTURES / "abi.json").read_text())
actual_abi = json.loads(subprocess.run([str(OUT / "package-test"), "--abi"], check=True,
                                     capture_output=True, text=True).stdout)
if actual_abi != expected_abi:
    raise SystemExit("C/TypeScript render ABI mismatch")
result = subprocess.run([str(OUT / "package-test"), *map(str, fixture_paths)],
                        check=True, capture_output=True, text=True, cwd=ROOT)
(OUT / "package-test.log").write_text(result.stdout + result.stderr)
print(result.stdout, end="")
subprocess.run(["gcov", "-j", "-b", "-c", "package-test-package.gcno"], check=True, cwd=OUT)
data = json.loads(gzip.decompress((OUT / "package-test-package.gcov.json.gz").read_bytes()))
module = next(file for file in data["files"] if file["file"].endswith("/render/package.c"))
lines, functions = module["lines"], module["functions"]
branches = [branch for line in lines for branch in line.get("branches", [])]
metrics = {
    "lines": 100 * sum(line["count"] > 0 for line in lines) / len(lines),
    "branches": 100 * sum(branch["count"] > 0 for branch in branches) / len(branches),
    "functions": 100 * sum(function["execution_count"] > 0 for function in functions) / len(functions),
}
(OUT / "coverage.json").write_text(json.dumps({"render-package": metrics}, indent=2) + "\n")
print(json.dumps(metrics, indent=2))
if any(value < 71 for value in metrics.values()):
    raise SystemExit("Render package inspector coverage below 71%")
