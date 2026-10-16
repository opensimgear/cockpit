"""Native fixed-memory diagnostics checks and existing per-module coverage gate."""
from pathlib import Path
import gzip, json, os, subprocess
root=Path(__file__).resolve().parents[3]
app=root/"apps/companion-esp32"
out=root/".research/esp32-diagnostics"
out.mkdir(parents=True,exist_ok=True)
for counter in out.glob("diagnostics-test-*.gcda"):
    counter.unlink()
subprocess.run([os.environ.get("CC","gcc"),"-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","--coverage","-fsanitize=address,undefined",
    str(app/"test/diagnostics_test.c"),"-o",str(out/"diagnostics-test")],check=True)
subprocess.run([str(out/"diagnostics-test")],check=True)
subprocess.run(["gcov","-j","-b","-c","diagnostics-test-diagnostics_test.gcno"],check=True,cwd=out,stdout=subprocess.DEVNULL)
data=json.loads(gzip.decompress((out/"diagnostics-test-diagnostics_test.gcov.json.gz").read_bytes()))
source=next(f for f in data["files"] if f["file"].replace("/../", "/").endswith("/core/diagnostics.c"))
lines,functions=source["lines"],source["functions"]
branches=[b for line in lines for b in line.get("branches",[])]
metrics={"lines":100*sum(l["count"]>0 for l in lines)/len(lines),"branches":100*sum(b["count"]>0 for b in branches)/len(branches),
    "functions":100*sum(f["execution_count"]>0 for f in functions)/len(functions)}
(out/"coverage.json").write_text(json.dumps(metrics,indent=2)+"\n")
print(json.dumps(metrics,indent=2))
if any(v<71 for v in metrics.values()):
    raise SystemExit("Diagnostics coverage below 71%")
