"""SDK-free ABI expression engine, independent TS vectors and module coverage."""
from pathlib import Path
import gzip
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/companion-esp32"
OUT = ROOT / ".research/esp32-render-expressions-native"
FIXTURES = ROOT / "packages/companion-renderer/test/fixtures/expressions.json"
OUT.mkdir(parents=True, exist_ok=True)
fixture = json.loads(FIXTURES.read_text())
assert fixture["formatVersion"] == 1 and fixture["q"] == 65536
# Generate test-only C statements from independent, checked-in expected values.
# This does not call the TypeScript evaluator or recompute its expectations.
lines = ["static void golden_vectors(void) {"]
for vector in fixture["vectors"]:
    assert len(vector["instructions"]) <= 1024 and len(vector["variables"]) <= 64
    assert len(vector["lookup"]) <= 2048
    lines.append("reset();")
    for index, instruction in enumerate(vector["instructions"]):
        lines.append(f'instruction({index}, {instruction["op"]}, {instruction.get("operand", 0)}, {instruction.get("value", 0)});')
    for index, value in enumerate(vector["variables"]):
        available = "true" if value is not None else "false"
        lines.append(f'sample({index}, OSG_EXPRESSION_NUMBER, {available}, (double)({value or 0}) / 65536.0);')
    for index, (x, y) in enumerate(vector["lookup"]):
        lines.append(f"point({index}, {x}, {y});")
    valid = "true" if vector["expected"] is not None else "false"
    name = json.dumps(vector["name"], ensure_ascii=True)
    lines.append(f'golden_expect({name}, {len(vector["instructions"])}, {valid}, {vector["expected"] or 0});')
lines.append("}")
(OUT / "expression-vectors.h").write_text("\n".join(lines) + "\n")
for counter in OUT.glob("expression-test-*.gcda"):
    counter.unlink()
subprocess.run([
    os.environ.get("CC", "gcc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic",
    "-g", "-fsanitize=address,undefined", "--coverage", "-I" + str(OUT),
    str(APP / "render/expressions.c"), str(APP / "test/expressions_test.c"), "-lm",
    "-o", str(OUT / "expression-test"),
], check=True, cwd=ROOT)
result = subprocess.run([str(OUT / "expression-test")], check=True, capture_output=True, text=True, cwd=ROOT)
(OUT / "expression-test.log").write_text(result.stdout + result.stderr)
print(result.stdout, end="")
print(f'Independent golden expressions: {len(fixture["vectors"])}')
subprocess.run(["gcov", "-j", "-b", "-c", "expression-test-expressions.gcno"], check=True, cwd=OUT)
data = json.loads(gzip.decompress((OUT / "expression-test-expressions.gcov.json.gz").read_bytes()))
module = next(file for file in data["files"] if file["file"].endswith("/render/expressions.c"))
code, functions = module["lines"], module["functions"]
branches = [branch for line in code for branch in line.get("branches", [])]
metrics = {
    "lines": 100 * sum(line["count"] > 0 for line in code) / len(code),
    "branches": 100 * sum(branch["count"] > 0 for branch in branches) / len(branches),
    "functions": 100 * sum(function["execution_count"] > 0 for function in functions) / len(functions),
}
(OUT / "coverage.json").write_text(json.dumps({"render-expressions": metrics}, indent=2) + "\n")
print(json.dumps(metrics, indent=2))
if any(value < 71 for value in metrics.values()):
    raise SystemExit("Expression evaluator coverage below 71%")