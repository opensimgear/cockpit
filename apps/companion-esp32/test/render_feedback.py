"""Compact renderer feedback protocol tests, sanitizers and a 71% coverage gate."""
from pathlib import Path
import gzip, json, os, subprocess
ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/companion-esp32"
OUT = ROOT / ".research/esp32-render-feedback"
OUT.mkdir(parents=True, exist_ok=True)
for p in OUT.glob("feedback-test-*.gcda"):
    p.unlink()
flags = [os.environ.get("CC", "gcc"), "-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic", "-g", "-fsanitize=address,undefined", "--coverage", "-DCJSON_NESTING_LIMIT=24", "-DCJSON_CIRCULAR_LIMIT=2048"]
flags += ["-I" + str(APP / p) for p in ("test/include", "core", "network", "third_party/cjson", "test")]
sources = ["core/companion.c", "core/diagnostics.c", "network/json.c", "network/publication.c", "network/render_feedback.c", "third_party/cjson/cJSON.c", "test/render_feedback_test.c"]
subprocess.run([*flags, *[str(APP / p) for p in sources], "-lm", "-o", str(OUT / "feedback-test")], check=True, cwd=ROOT)
subprocess.run([str(OUT / "feedback-test")], check=True, cwd=ROOT)
subprocess.run(["gcov", "-j", "-b", "-c", "feedback-test-render_feedback.gcno"], check=True, cwd=OUT, stdout=subprocess.DEVNULL)
data = json.loads(gzip.decompress((OUT / "feedback-test-render_feedback.gcov.json.gz").read_bytes()))
source = next(f for f in data["files"] if f["file"].endswith("/network/render_feedback.c"))
lines, functions = source["lines"], source["functions"]
branches = [b for line in lines for b in line.get("branches", [])]
metrics = {"lines":100*sum(l["count"]>0 for l in lines)/len(lines), "branches":100*sum(b["count"]>0 for b in branches)/len(branches), "functions":100*sum(f["execution_count"]>0 for f in functions)/len(functions)}
(OUT / "coverage.json").write_text(json.dumps(metrics, indent=2) + "\n")
print(json.dumps(metrics, indent=2))
assert all(v >= 71 for v in metrics.values()), metrics