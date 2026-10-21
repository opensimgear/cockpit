"""Portable raw-flash A/B content store: real SHA256 and power-cut fault injection."""
from pathlib import Path
import gzip, json, os, subprocess
ROOT=Path(__file__).resolve().parents[3]
APP=ROOT/"apps/companion-esp32"
OUT=ROOT/".research/esp32-content-store-native"
OUT.mkdir(parents=True,exist_ok=True)
for counter in OUT.glob("content-store-test-*.gcda"):
    counter.unlink()
subprocess.run([os.environ.get("CC","gcc"),"-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","--coverage","-fsanitize=address,undefined",
    str(APP/"render/content_store.c"),str(APP/"test/content_store_test.c"),"-lcrypto","-o",str(OUT/"content-store-test")],check=True)
result=subprocess.run([str(OUT/"content-store-test")],check=False,capture_output=True,text=True,timeout=60)
(OUT/"content-store-test.log").write_text(result.stdout+result.stderr)
print(result.stdout+result.stderr,end="")
result.check_returncode()
subprocess.run(["gcov","-j","-b","-c","content-store-test-content_store.gcno"],check=True,cwd=OUT,stdout=subprocess.DEVNULL)
data=json.loads(gzip.decompress((OUT/"content-store-test-content_store.gcov.json.gz").read_bytes()))
source=next(f for f in data["files"] if f["file"].endswith("/render/content_store.c"))
lines,functions=source["lines"],source["functions"]
branches=[b for line in lines for b in line.get("branches",[])]
metrics={"lines":100*sum(l["count"]>0 for l in lines)/len(lines),"branches":100*sum(b["count"]>0 for b in branches)/len(branches),"functions":100*sum(f["execution_count"]>0 for f in functions)/len(functions)}
(OUT/"coverage.json").write_text(json.dumps(metrics,indent=2)+"\n")
print(json.dumps(metrics,indent=2))
if any(v<71 for v in metrics.values()):
    raise SystemExit("Content-store coverage below 71%")
