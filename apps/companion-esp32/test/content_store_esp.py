"""SDK-mock tests for the guarded ESP partition/SHA adapter, without hardware."""
from pathlib import Path
import gzip,json,os,subprocess
ROOT=Path(__file__).resolve().parents[3];APP=ROOT/"apps/companion-esp32";OUT=ROOT/".research/esp32-content-store-esp-native"
OUT.mkdir(parents=True,exist_ok=True)
for counter in OUT.glob("content-esp-test-*.gcda"):counter.unlink()
subprocess.run([os.environ.get("CC","gcc"),"-std=c11","-Wall","-Wextra","-Werror","-pedantic","-g","--coverage","-fsanitize=address,undefined","-I"+str(APP/"test/content_store_include"),str(APP/"test/content_store_esp_test.c"),"-o",str(OUT/"content-esp-test")],check=True)
subprocess.run([str(OUT/"content-esp-test")],check=True)
subprocess.run(["gcov","-j","-b","-c","content-esp-test-content_store_esp_test.gcno"],check=True,cwd=OUT,stdout=subprocess.DEVNULL)
data=json.loads(gzip.decompress((OUT/"content-esp-test-content_store_esp_test.gcov.json.gz").read_bytes()))
source=next(f for f in data["files"] if f["file"].replace("/../","/").endswith("/render/content_store_esp.c"));lines,functions=source["lines"],source["functions"];branches=[b for l in lines for b in l.get("branches",[])]
metrics={"lines":100*sum(l["count"]>0 for l in lines)/len(lines),"branches":100*sum(b["count"]>0 for b in branches)/len(branches),"functions":100*sum(f["execution_count"]>0 for f in functions)/len(functions)}
(OUT/"coverage.json").write_text(json.dumps(metrics,indent=2)+"\n");print(json.dumps(metrics,indent=2))
if any(v<71 for v in metrics.values()):raise SystemExit("ESP content adapter coverage below71%")
